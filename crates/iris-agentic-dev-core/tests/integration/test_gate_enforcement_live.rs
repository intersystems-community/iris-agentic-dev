// Spec 085 write-gate integrity — US1 enforcement, against live IRIS (T016, T017, T018).
//
// The point of every test here is the **negative side effect**: after a refusal, the global /
// document / lookup entry / table must not exist in IRIS. "The error code came back" is what the
// existing tests assert and it is exactly what let the reporter's bypasses ship — a tool can
// return WRITE_TOOLS_DISABLED from one code path while another path has already written.
//
// The gate is checked in `ServerHandler::call_tool`, so it is only reachable through a real MCP
// session. These tests spawn the built binary, point `OBJECTSCRIPT_WORKSPACE` at a tempdir holding
// `.iris-agentic-dev.toml`, and speak JSON-RPC over stdio. Calling the handler impls directly (as
// `test_dispatch_gate_handlers.rs` does) cannot exercise it.
//
// Run with:
//   IRIS_HOST=localhost IRIS_WEB_PORT=52780 \
//     cargo test -p iris-agentic-dev-core --features testing \
//       --test test_gate_enforcement_live -- --ignored --test-threads=1 --nocapture

use std::collections::BTreeSet;
use std::io::{BufRead, BufReader, Write};
use std::process::{Child, ChildStdin, ChildStdout, Command, Stdio};

use iris_agentic_dev_core::tools::write_gate::{
    WriteClass, CLASSIFICATION, ERR_DESTRUCTIVE_GATE, ERR_WRITE_GATE,
};
use serde_json::{json, Value};

/// Every global these tests touch lives under this prefix, so a run that dies mid-way leaves one
/// obvious thing to kill and can never collide with real data (T075).
const PROBE: &str = "IADGate085";

/// The gate the reporter had set when the bypasses were verified: declared off in the config file,
/// nothing else declared.
const GATE_OFF: &str = "write_tools_enabled = false\n";

/// `global_kill` deletes a whole global, so the destructive matrix needs one of its own rather than
/// a subscript of `PROBE` — killing `PROBE` would take the other tests' state with it. Same prefix,
/// so the T075 sweep still finds it.
const KILL_PROBE: &str = "IADGate085Kill";

/// The saved server T045 seeds into an isolated home directory and then fails to remove.
const PROBE_SERVER: &str = "iadgate085srv";

/// The `^SKILLS` subscript T045 seeds and then fails to forget.
const PROBE_SKILL: &str = "IADGate085Skill";

/// Writes on, destructive tier left undeclared — which means off, because the tier is never
/// inferred (data-model.md §1, `InferredDefault`).
const GATE_ON_WRITES_ONLY: &str = "write_tools_enabled = true\n";

/// Both tiers declared on.
const GATE_ON_BOTH: &str = "write_tools_enabled = true\ndestructive_tools_enabled = true\n";

// ── harness ───────────────────────────────────────────────────────────────────

fn iris_dev_bin() -> std::path::PathBuf {
    if let Ok(path) = std::env::var("IRIS_DEV_BIN") {
        let p = std::path::PathBuf::from(path);
        if p.exists() {
            return p;
        }
    }
    let workspace_root = {
        let mut p = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"));
        p.pop();
        p.pop();
        p
    };
    for target_subdir in [
        "target/debug/iris-agentic-dev",
        "target/release/iris-agentic-dev",
        "target/llvm-cov-target/debug/iris-agentic-dev",
        "target/llvm-cov-target/release/iris-agentic-dev",
    ] {
        let candidate = workspace_root.join(target_subdir);
        if candidate.exists() {
            return candidate;
        }
    }
    workspace_root.join("target/debug/iris-agentic-dev")
}

/// These tests need a live container. They skip without one rather than fail, but they never skip
/// silently past a *missing binary* — a harness that cannot spawn the server would otherwise
/// "pass" while asserting nothing, which is the failure mode this whole feature exists to remove.
fn no_iris() -> bool {
    if std::env::var("IRIS_HOST").unwrap_or_default().is_empty() {
        eprintln!("Skipping: IRIS_HOST not set");
        return true;
    }
    false
}

/// One MCP server process, kept alive across calls.
///
/// Persistent by necessity, not convenience: `iris_ws_open` → `iris_ws_exec` only means anything
/// inside one process, and every read-back has to happen in the same gate-off session it is
/// checking, or it proves nothing about that session.
struct Mcp {
    child: Child,
    stdin: ChildStdin,
    reader: BufReader<ChildStdout>,
    next_id: u64,
    /// Holds the config-file tempdir open for the process's lifetime.
    _dir: tempfile::TempDir,
}

impl Drop for Mcp {
    fn drop(&mut self) {
        self.child.kill().ok();
        self.child.wait().ok();
    }
}

impl Mcp {
    fn start(config_toml: &str) -> Mcp {
        Mcp::start_inner(config_toml, None)
    }

    /// Spawn with the process's home directory redirected at `home`.
    ///
    /// For the destructive items whose target is local state rather than IRIS: `iris_remove_server`
    /// rewrites `~/.config/iris-agentic-dev/servers.json` and deletes an OS keychain entry, so a
    /// test run against the developer's real registry would either destroy a server they use or
    /// pass for the wrong reason — the entry it looked for was never there.
    fn start_with_home(config_toml: &str, home: &std::path::Path) -> Mcp {
        Mcp::start_inner(config_toml, Some(home))
    }

    fn start_inner(config_toml: &str, home: Option<&std::path::Path>) -> Mcp {
        let dir = tempfile::tempdir().expect("tempdir");
        std::fs::write(dir.path().join(".iris-agentic-dev.toml"), config_toml).expect("write toml");

        let bin = iris_dev_bin();
        assert!(
            bin.exists(),
            "server binary not found at {} — build it first: \
             cargo build -p iris-agentic-dev --bin iris-agentic-dev",
            bin.display()
        );

        let mut cmd = Command::new(&bin);
        cmd.arg("mcp");
        // Operator env outranks the config file (FR-003), so a developer who has any of these
        // exported would have their shell measured instead of the toml under test.
        for key in [
            "IRIS_WRITE_TOOLS_ENABLED",
            "IRIS_DESTRUCTIVE_TOOLS_ENABLED",
            "IRIS_ALLOW_PROD",
            "IRIS_CONTAINER",
            "IRIS_ENABLED_TOOLS",
            "IRIS_DISABLED_TOOLS",
            // Not a gate, but `skill` short-circuits to LEARNING_DISABLED when this is "false",
            // which would answer the skill rows before the tier ever decided anything.
            "OBJECTSCRIPT_LEARNING",
        ] {
            cmd.env_remove(key);
        }
        for key in [
            "IRIS_HOST",
            "IRIS_WEB_PORT",
            "IRIS_USERNAME",
            "IRIS_PASSWORD",
            "IRIS_NAMESPACE",
        ] {
            if let Ok(v) = std::env::var(key) {
                cmd.env(key, v);
            }
        }
        cmd.env("OBJECTSCRIPT_WORKSPACE", dir.path());
        if let Some(h) = home {
            // `servers_config::native_config_path()` reads `$HOME` on Unix and `%APPDATA%` on
            // Windows, so isolating one platform is not isolating the test.
            cmd.env("HOME", h);
            cmd.env("USERPROFILE", h);
            cmd.env("APPDATA", h.join("AppData").join("Roaming"));
        }
        // `merged` is already the default. Pinned because the refusal matrix is an intersection
        // with the live tool list, and a different toolset would silently shrink it.
        cmd.env("IRIS_TOOLSET", "merged");

        let mut child = cmd
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
            .spawn()
            .expect("spawn iris-agentic-dev mcp");
        let stdin = child.stdin.take().expect("stdin");
        let stdout = child.stdout.take().expect("stdout");

        let mut mcp = Mcp {
            child,
            stdin,
            reader: BufReader::new(stdout),
            next_id: 1,
            _dir: dir,
        };
        mcp.request(
            "initialize",
            json!({
                "protocolVersion": "2024-11-05",
                "capabilities": {},
                "clientInfo": {"name": "gate-085", "version": "0.1"}
            }),
        );
        mcp.notify("notifications/initialized", json!({}));
        mcp
    }

    fn send(&mut self, msg: &Value) {
        let line = serde_json::to_string(msg).expect("serialize");
        self.stdin
            .write_all(format!("{line}\n").as_bytes())
            .expect("write to server stdin");
        self.stdin.flush().expect("flush");
    }

    fn notify(&mut self, method: &str, params: Value) {
        self.send(&json!({"jsonrpc": "2.0", "method": method, "params": params}));
    }

    /// Send a request and return the matching response, skipping any notifications the server
    /// interleaves.
    fn request(&mut self, method: &str, params: Value) -> Value {
        let id = self.next_id;
        self.next_id += 1;
        self.send(&json!({"jsonrpc": "2.0", "id": id, "method": method, "params": params}));
        loop {
            let mut line = String::new();
            match self.reader.read_line(&mut line) {
                Ok(0) => panic!("server closed stdout while waiting for {method} (id {id})"),
                Ok(_) => {
                    if let Ok(v) = serde_json::from_str::<Value>(&line) {
                        if v["id"] == json!(id) {
                            return v;
                        }
                    }
                }
                Err(e) => panic!("reading response to {method} (id {id}): {e}"),
            }
        }
    }

    /// A tool call, as the payload the tool returned. `Value::Null` when the call produced a
    /// JSON-RPC error instead of a result — which is itself a failure for every assertion here,
    /// because a gated call must come back as a normal refusal envelope (Principle V).
    fn call(&mut self, tool: &str, args: Value) -> Value {
        let resp = self.request("tools/call", json!({"name": tool, "arguments": args}));
        let structured = resp["result"]["structuredContent"].clone();
        if structured.is_object() {
            return structured;
        }
        resp["result"]["content"]
            .as_array()
            .and_then(|a| a.first())
            .and_then(|c| c["text"].as_str())
            .and_then(|t| serde_json::from_str::<Value>(t).ok())
            .unwrap_or(Value::Null)
    }

    /// Every tool the session actually advertises, following `nextCursor` to the end.
    fn tool_names(&mut self) -> BTreeSet<String> {
        let mut names = BTreeSet::new();
        let mut cursor: Option<String> = None;
        loop {
            let params = match &cursor {
                Some(c) => json!({"cursor": c}),
                None => json!({}),
            };
            let resp = self.request("tools/list", params);
            for t in resp["result"]["tools"]
                .as_array()
                .cloned()
                .unwrap_or_default()
            {
                if let Some(n) = t["name"].as_str() {
                    names.insert(n.to_string());
                }
            }
            match resp["result"]["nextCursor"].as_str() {
                Some(c) => cursor = Some(c.to_string()),
                None => return names,
            }
        }
    }
}

fn assert_refused(payload: &Value, expected_code: &str, what: &str) {
    assert_eq!(
        payload["error_code"].as_str().unwrap_or("<absent>"),
        expected_code,
        "{what}: expected {expected_code}, got {payload}"
    );
    assert_eq!(
        payload["success"],
        json!(false),
        "{what}: a refusal must report success=false, got {payload}"
    );
}

/// The assertion the whole file exists for: the probe global is not there afterwards.
///
/// The read-back runs in the same gate-off session, so it doubles as evidence that read-only
/// tools keep working (T018) — if `iris_global` `get` were itself gated, this would fail loudly
/// rather than quietly reporting "not defined".
fn assert_probe_absent(mcp: &mut Mcp, subscript: &str) {
    let got = mcp.call(
        "iris_global",
        json!({"action": "get", "global_name": PROBE, "subscripts": [subscript]}),
    );
    assert_eq!(
        got["success"],
        json!(true),
        "the read-back must itself succeed with the gate off — iris_global get is read-only: {got}"
    );
    assert_eq!(
        got["defined"],
        json!(false),
        "^{PROBE}(\"{subscript}\") exists, so the refusal did not prevent the write: {got}"
    );
}

fn probe_get(mcp: &mut Mcp, subscript: &str) -> Value {
    mcp.call(
        "iris_global",
        json!({"action": "get", "global_name": PROBE, "subscripts": [subscript]}),
    )
}

/// Say out loud that a row's surviving-target check could not run.
///
/// Some destructive targets need features this instance may not have — Ensemble lookup tables and
/// Ensemble credentials are absent from `USER` on a plain Community build. The refusal itself is
/// still asserted (the gate is decided before dispatch, so it needs no IRIS at all), but the
/// "target is still there afterwards" half is the half that matters, and a test that quietly drops
/// it is the failure mode this whole feature exists to remove.
fn degraded(what: &str, detail: &Value) {
    eprintln!(
        "DEGRADED: {what} could not be prepared on this instance: {detail}\n\
         The refusal is still asserted; the surviving-target check for that row is NOT. Do not read \
         this run as covering it — fix the instance instead."
    );
}

// ── T016: the refusal matrix ──────────────────────────────────────────────────

/// Driven off `CLASSIFICATION` rather than a hand-written list, so a write-capable tool added
/// without a gate fails CI (FR-010, FR-026, SC-001).
///
/// Intersected with the live `tools/list`, because the `merged` toolset prunes roughly twenty
/// names — asserting against `CLASSIFICATION` alone would probe tools this session does not have.
#[test]
#[ignore]
fn t016_every_write_capable_tool_is_refused_with_the_gate_off() {
    if no_iris() {
        return;
    }
    let mut mcp = Mcp::start(GATE_OFF);
    let live = mcp.tool_names();
    assert!(
        live.len() > 40,
        "tools/list returned only {} tools — the session did not come up",
        live.len()
    );

    let mut probed = 0usize;
    for entry in CLASSIFICATION {
        if !live.contains(entry.tool) {
            continue;
        }
        let mut probes: Vec<(String, Value)> = vec![];
        if entry.default != WriteClass::ReadOnly {
            probes.push((format!("{} (no action)", entry.tool), json!({})));
        }
        for (action, class) in entry.actions {
            if *class != WriteClass::ReadOnly {
                probes.push((
                    format!("{}(action={action})", entry.tool),
                    json!({"action": action}),
                ));
            }
        }
        for (label, args) in probes {
            // Both tiers report the write gate here: writes off is the more fundamental refusal
            // and the one whose remedy comes first (data-model.md §3).
            let payload = mcp.call(entry.tool, args);
            assert_refused(&payload, ERR_WRITE_GATE, &label);
            probed += 1;
        }
    }
    assert!(
        probed >= 30,
        "the matrix probed only {probed} calls — the intersection with the live tool list \
         collapsed, so this test asserted almost nothing"
    );
    eprintln!("t016: {probed} write-capable calls refused");
}

/// The five bypasses the reporter verified against 1.2.6, each followed by the read-back that
/// proves nothing was written (FR-025).
#[test]
#[ignore]
fn t016_refused_writes_leave_nothing_behind_in_iris() {
    if no_iris() {
        return;
    }
    let mut mcp = Mcp::start(GATE_OFF);

    // iris_global set — ungated in 1.2.6.
    let refused = mcp.call(
        "iris_global",
        json!({"action": "set", "global_name": PROBE, "subscripts": ["global"], "value": "1"}),
    );
    assert_refused(&refused, ERR_WRITE_GATE, "iris_global set");
    assert_probe_absent(&mut mcp, "global");

    // iris_execute — gated in 1.2.6, and it must stay gated once the guard moves upstream.
    let refused = mcp.call(
        "iris_execute",
        json!({"code": format!("Set ^{PROBE}(\"exec\")=1")}),
    );
    assert_refused(&refused, ERR_WRITE_GATE, "iris_execute");
    assert_probe_absent(&mut mcp, "exec");

    // iris_execute_method — ungated in 1.2.6. %SYSTEM.OBJ has no such method, so if this were
    // ever to run it would error rather than write; the assertion is the refusal itself.
    let refused = mcp.call(
        "iris_execute_method",
        json!({
            "class_name": "%SYSTEM.OBJ",
            "method_name": "Compile",
            "arguments": [format!("{PROBE}.Nothing.cls")]
        }),
    );
    assert_refused(&refused, ERR_WRITE_GATE, "iris_execute_method");

    // iris_doc put — gated in 1.2.6. Absence is asserted with mode=head, which is read-only.
    let doc = format!("{PROBE}.Probe.cls");
    let refused = mcp.call(
        "iris_doc",
        json!({
            "mode": "put",
            "name": doc,
            "content": format!("Class {PROBE}.Probe Extends %RegisteredObject\n{{\n}}\n"),
        }),
    );
    assert_refused(&refused, ERR_WRITE_GATE, "iris_doc put");
    let head = mcp.call("iris_doc", json!({"mode": "head", "name": doc}));
    assert_eq!(
        head["success"],
        json!(true),
        "iris_doc head is read-only and must still work: {head}"
    );
    assert_eq!(
        head["exists"],
        json!(false),
        "{doc} exists, so the refused put wrote anyway: {head}"
    );

    // iris_lookup_manage set — ungated in 1.2.6.
    let refused = mcp.call(
        "iris_lookup_manage",
        json!({"action": "set", "table": PROBE, "key": "k", "value": "v"}),
    );
    assert_refused(&refused, ERR_WRITE_GATE, "iris_lookup_manage set");
    let got = mcp.call(
        "iris_lookup_manage",
        json!({"action": "get", "table": PROBE, "key": "k"}),
    );
    assert_eq!(
        got["success"],
        json!(false),
        "the lookup entry exists, so the refused set wrote anyway: {got}"
    );
    let code = got["error_code"].as_str().unwrap_or("<absent>");
    assert!(
        code == "TABLE_NOT_FOUND" || code == "KEY_NOT_FOUND",
        "expected the entry to be missing, got error_code {code}: {got}"
    );

    // iris_query mode=write — gated in 1.2.6. Absence is asserted through the catalog.
    let refused = mcp.call(
        "iris_query",
        json!({
            "mode": "write",
            "confirm": true,
            "query": format!("CREATE TABLE {PROBE}.Probe (Id INT)"),
        }),
    );
    assert_refused(&refused, ERR_WRITE_GATE, "iris_query mode=write");
    let count = mcp.call(
        "iris_query",
        json!({
            "query": format!(
                "SELECT COUNT(*) AS Tables FROM INFORMATION_SCHEMA.TABLES \
                 WHERE TABLE_SCHEMA = '{PROBE}'"
            )
        }),
    );
    assert_eq!(
        count["success"],
        json!(true),
        "a read query must still work with the gate off: {count}"
    );
    let tables = count["rows"][0]["Tables"].clone();
    assert!(
        tables == json!(0) || tables == json!("0"),
        "schema {PROBE} has tables, so the refused CREATE TABLE ran anyway: {count}"
    );
}

// ── T017: the terminal-session bypass ─────────────────────────────────────────

/// `iris_ws_open` then `iris_ws_exec` ran arbitrary ObjectScript against 1.2.6 with the gate
/// provably active — the complete bypass of the `iris_execute` gate, and the highest-severity
/// finding in the report.
///
/// Opening and closing a session mutates nothing, so both stay read-only; the gate belongs on
/// `exec`. Where the build has no WS terminal (Atelier below V7) the session token is irrelevant:
/// the gate is checked before the session is looked up, which is the property under test.
#[test]
#[ignore]
fn t017_ws_exec_is_gated_and_the_terminal_writes_nothing() {
    if no_iris() {
        return;
    }
    let mut mcp = Mcp::start(GATE_OFF);

    let opened = mcp.call("iris_ws_open", json!({}));
    let session = opened["session"]
        .as_str()
        .unwrap_or("no-such-session-085")
        .to_string();

    let refused = mcp.call(
        "iris_ws_exec",
        json!({"session": session, "code": format!("Set ^{PROBE}(\"ws\")=1")}),
    );
    assert_refused(&refused, ERR_WRITE_GATE, "iris_ws_exec");
    assert_probe_absent(&mut mcp, "ws");

    if opened["session"].is_string() {
        mcp.call("iris_ws_close", json!({"session": session}));
    }
}

// ── T018: reads keep working, and the gate on still writes ────────────────────

/// The complement to the refusal matrix (Constitution IV): a gate that refuses everything is not
/// the feature. Read-only tools must be unaffected with the gate off.
#[test]
#[ignore]
fn t018_read_only_tools_still_work_with_the_gate_off() {
    if no_iris() {
        return;
    }
    let mut mcp = Mcp::start(GATE_OFF);

    let cfg = mcp.call("check_config", json!({}));
    assert_eq!(
        cfg["write_tools_enabled"],
        json!(false),
        "check_config must report the gate the config declared: {cfg}"
    );

    let namespaces = mcp.call("iris_namespace_list", json!({}));
    assert_eq!(
        namespaces["success"],
        json!(true),
        "iris_namespace_list is read-only: {namespaces}"
    );
    assert!(
        namespaces["count"].as_i64().unwrap_or(0) > 0,
        "expected at least one namespace: {namespaces}"
    );

    let query = mcp.call("iris_query", json!({"query": "SELECT 1 AS One"}));
    assert_eq!(
        query["success"],
        json!(true),
        "a read query is read-only: {query}"
    );

    // Local state rather than IRIS, so the gate has a second kind of read to get wrong. This
    // envelope has no `success` field — it answers with the list itself — so the registered
    // connection is what proves the call went through rather than being refused.
    let servers = mcp.call("iris_servers", json!({}));
    assert!(
        servers["servers"]
            .as_array()
            .is_some_and(|s| !s.is_empty() && s.iter().all(|e| e["name"].is_string())),
        "iris_servers is read-only and must return the registered instances: {servers}"
    );
    assert!(
        servers["error_code"].is_null(),
        "iris_servers must not be refused with the gate off: {servers}"
    );
}

/// Three sessions over one probe global, covering the whole ladder:
///
/// 1. writes on, tier off — the write succeeds, the kill is refused with
///    `DESTRUCTIVE_TOOLS_DISABLED`, and the global survives (FR-018);
/// 2. writes off — the same kill reports `WRITE_TOOLS_DISABLED`, not the tier error, because the
///    write gate is the more fundamental refusal (US7 scenario 3);
/// 3. both declared on — the kill goes through, which is also this file's cleanup.
#[test]
#[ignore]
fn t018_destructive_tier_gates_the_kill_and_the_global_survives() {
    if no_iris() {
        return;
    }

    {
        let mut mcp = Mcp::start(GATE_ON_WRITES_ONLY);
        let set = mcp.call(
            "iris_global",
            json!({"action": "set", "global_name": PROBE, "subscripts": ["survivor"], "value": "1"}),
        );
        assert_eq!(
            set["success"],
            json!(true),
            "the gate declared on must actually permit the write: {set}"
        );
        let got = probe_get(&mut mcp, "survivor");
        assert_eq!(got["defined"], json!(true), "seed did not land: {got}");

        let refused = mcp.call(
            "iris_global",
            json!({"action": "kill", "global_name": PROBE, "subscripts": ["survivor"]}),
        );
        assert_refused(&refused, ERR_DESTRUCTIVE_GATE, "iris_global kill, tier off");
        let got = probe_get(&mut mcp, "survivor");
        assert_eq!(
            got["defined"],
            json!(true),
            "the refused kill deleted the global anyway: {got}"
        );
    }

    {
        let mut mcp = Mcp::start(GATE_OFF);
        let refused = mcp.call(
            "iris_global",
            json!({"action": "kill", "global_name": PROBE, "subscripts": ["survivor"]}),
        );
        assert_refused(&refused, ERR_WRITE_GATE, "iris_global kill, writes off");
        let got = probe_get(&mut mcp, "survivor");
        assert_eq!(
            got["defined"],
            json!(true),
            "the refused kill deleted the global anyway: {got}"
        );
    }

    {
        let mut mcp = Mcp::start(GATE_ON_BOTH);
        let killed = mcp.call(
            "iris_global",
            json!({"action": "kill", "global_name": PROBE, "subscripts": ["survivor"]}),
        );
        assert_eq!(
            killed["success"],
            json!(true),
            "both tiers declared on must permit the kill: {killed}"
        );
        let got = probe_get(&mut mcp, "survivor");
        assert_eq!(
            got["defined"],
            json!(false),
            "the permitted kill did not delete the global: {got}"
        );
    }
}

// ── T044: the destructive-tier refusal matrix ─────────────────────────────────

/// Writes on, destructive tier undeclared — every destructive item with an observable target in
/// IRIS is refused with `DESTRUCTIVE_TOOLS_DISABLED`, and the target is still there afterwards
/// (FR-025, SC-009).
///
/// The five rows here are the ones IRIS can be asked about. Two of the seven are local state —
/// `iris_remove_server` and `skill(action="forget")` — and live in `t045_…` because their surviving
/// artifact is on disk, not in the instance.
///
/// Three of these five *create* rather than destroy: `iris_namespace_create`, `iris_admin
/// create_user`, `iris_credential_manage create`. They are in the destructive tier because a
/// namespace, a user and a credential are not things an agent should be able to conjure on a live
/// instance, so for those the surviving target is inverted — the thing must be **absent**.
///
/// `global_kill` is driven with a real `confirm_token` minted by `global_preview` in the same
/// session. Without it the call would be refused for a missing token and the test would pass while
/// proving nothing about the tier.
#[test]
#[ignore]
fn t044_destructive_tier_refused_and_every_target_survives() {
    if no_iris() {
        return;
    }

    let probe_user = "IADGate085Usr";
    let probe_ns = "IADGATE085NS";
    let probe_cred = "IADGate085Cred";
    let lookup_key = "t044";

    // ── Seed, with both tiers on ──────────────────────────────────────────────
    let lookup_seeded = {
        let mut mcp = Mcp::start(GATE_ON_BOTH);
        let set = mcp.call(
            "iris_global",
            json!({"action": "set", "global_name": KILL_PROBE, "subscripts": ["x"], "value": "1"}),
        );
        assert_eq!(
            set["success"],
            json!(true),
            "could not seed ^{KILL_PROBE} with both tiers on — nothing below would mean anything: \
             {set}"
        );

        let seeded = mcp.call(
            "iris_lookup_manage",
            json!({"action": "set", "table": PROBE, "key": lookup_key, "value": "v"}),
        );
        if seeded["success"] == json!(true) {
            true
        } else {
            degraded("the Ensemble lookup entry", &seeded);
            false
        }
    };

    // ── The matrix: writes on, tier undeclared ────────────────────────────────
    {
        let mut mcp = Mcp::start(GATE_ON_WRITES_ONLY);

        // Undeclared means off, and check_config has to say which input decided that — otherwise
        // an operator sees a refusal they did not ask for and has to guess (FR-019).
        let cfg = mcp.call("check_config", json!({}));
        assert_eq!(
            cfg["destructive_tools_enabled"],
            json!(false),
            "the tier is never inferred, so undeclared must report false: {cfg}"
        );
        assert!(
            cfg["destructive_tools_source"]
                .as_str()
                .is_some_and(|s| !s.is_empty()),
            "the refusal below needs a named source, or it is unattributable: {cfg}"
        );

        // Ordinary writes are on. This is what separates the tier from a blanket refusal: without
        // it, a gate stuck fully closed would satisfy every assertion that follows.
        let ordinary = mcp.call(
            "iris_global",
            json!({"action": "set", "global_name": PROBE, "subscripts": [lookup_key], "value": "1"}),
        );
        assert_eq!(
            ordinary["success"],
            json!(true),
            "writes are declared on, so an ordinary write must land: {ordinary}"
        );

        // 1. global_kill, with a valid token — so the tier is the only thing left refusing it.
        let preview = mcp.call("global_preview", json!({"global": KILL_PROBE}));
        assert_eq!(
            preview["success"],
            json!(true),
            "global_preview is read-only and must work with the tier off: {preview}"
        );
        let token = preview["confirm_token"]
            .as_str()
            .expect("global_preview must mint a confirm_token")
            .to_string();
        let refused = mcp.call(
            "global_kill",
            json!({"global": KILL_PROBE, "confirm_token": token}),
        );
        assert_refused(
            &refused,
            ERR_DESTRUCTIVE_GATE,
            "global_kill with a valid token",
        );
        let got = mcp.call(
            "iris_global",
            json!({"action": "get", "global_name": KILL_PROBE, "subscripts": ["x"]}),
        );
        assert_eq!(
            got["defined"],
            json!(true),
            "^{KILL_PROBE} is gone, so the refused global_kill killed it anyway: {got}"
        );

        // 2. iris_lookup_manage delete — the entry survives, and get still reads.
        let refused = mcp.call(
            "iris_lookup_manage",
            json!({"action": "delete", "table": PROBE, "key": lookup_key}),
        );
        assert_refused(&refused, ERR_DESTRUCTIVE_GATE, "iris_lookup_manage delete");
        let got = mcp.call(
            "iris_lookup_manage",
            json!({"action": "get", "table": PROBE, "key": lookup_key}),
        );
        if lookup_seeded {
            assert_eq!(
                got["success"],
                json!(true),
                "the seeded entry is gone, so the refused delete deleted it: {got}"
            );
        } else {
            // Still worth an assertion: get is a read action and must not be gated at all.
            assert_ne!(
                got["error_code"].as_str().unwrap_or("<absent>"),
                ERR_DESTRUCTIVE_GATE,
                "iris_lookup_manage get is read-only and must never hit the tier gate: {got}"
            );
        }

        // 3. iris_namespace_create — inverted target: the namespace must not appear.
        let refused = mcp.call("iris_namespace_create", json!({"name": probe_ns}));
        assert_refused(&refused, ERR_DESTRUCTIVE_GATE, "iris_namespace_create");
        let namespaces = mcp.call("iris_namespace_list", json!({}));
        assert_eq!(
            namespaces["success"],
            json!(true),
            "iris_namespace_list is read-only and must answer: {namespaces}"
        );
        assert!(
            !serde_json::to_string(&namespaces)
                .unwrap_or_default()
                .contains(probe_ns),
            "{probe_ns} exists, so the refused iris_namespace_create created it: {namespaces}"
        );

        // 4. iris_admin create_user — inverted target: the user must not appear in list_users.
        let refused = mcp.call(
            "iris_admin",
            json!({"action": "create_user", "username": probe_user, "password": "IadGate085!x"}),
        );
        assert_refused(&refused, ERR_DESTRUCTIVE_GATE, "iris_admin create_user");
        let users = mcp.call("iris_admin", json!({"action": "list_users"}));
        if users["success"] == json!(true) {
            assert!(
                !serde_json::to_string(&users)
                    .unwrap_or_default()
                    .contains(probe_user),
                "{probe_user} exists, so the refused create_user created it: {users}"
            );
        } else {
            degraded("the iris_admin list_users read-back", &users);
        }

        // 5. iris_credential_manage create — inverted target, Ensemble-dependent.
        let refused = mcp.call(
            "iris_credential_manage",
            json!({
                "action": "create",
                "id": probe_cred,
                "username": "iad085",
                "password": "IadGate085!x",
            }),
        );
        assert_refused(
            &refused,
            ERR_DESTRUCTIVE_GATE,
            "iris_credential_manage create",
        );
        let creds = mcp.call("iris_credential_list", json!({}));
        if creds["success"] == json!(true) {
            assert!(
                !serde_json::to_string(&creds)
                    .unwrap_or_default()
                    .contains(probe_cred),
                "{probe_cred} exists, so the refused create created it: {creds}"
            );
        } else {
            degraded("the iris_credential_list read-back", &creds);
        }
    }

    // ── Both tiers on: the same operations go through, and this is the cleanup ─
    //
    // Without this the five refusals above would be satisfied by a destructive tier that never
    // opens — the surviving targets would survive because nothing can touch them at all.
    {
        let mut mcp = Mcp::start(GATE_ON_BOTH);

        let preview = mcp.call("global_preview", json!({"global": KILL_PROBE}));
        let token = preview["confirm_token"]
            .as_str()
            .expect("global_preview must mint a confirm_token")
            .to_string();
        let killed = mcp.call(
            "global_kill",
            json!({"global": KILL_PROBE, "confirm_token": token}),
        );
        assert_eq!(
            killed["success"],
            json!(true),
            "both tiers on must permit global_kill: {killed}"
        );
        let got = mcp.call(
            "iris_global",
            json!({"action": "get", "global_name": KILL_PROBE, "subscripts": ["x"]}),
        );
        assert_eq!(
            got["defined"],
            json!(false),
            "the permitted global_kill left ^{KILL_PROBE} behind: {got}"
        );

        if lookup_seeded {
            let deleted = mcp.call(
                "iris_lookup_manage",
                json!({"action": "delete", "table": PROBE, "key": lookup_key}),
            );
            assert_eq!(
                deleted["success"],
                json!(true),
                "both tiers on must permit the lookup delete: {deleted}"
            );
        }

        mcp.call(
            "iris_global",
            json!({"action": "kill", "global_name": PROBE, "subscripts": [lookup_key]}),
        );
    }
}

// ── T045: the destructive items whose target is local state ───────────────────

/// Where `servers_config::native_config_path()` will look, given a redirected home directory.
fn seeded_servers_path(home: &std::path::Path) -> std::path::PathBuf {
    #[cfg(target_os = "windows")]
    let base = home.join("AppData").join("Roaming");
    #[cfg(not(target_os = "windows"))]
    let base = home.join(".config");
    base.join("iris-agentic-dev").join("servers.json")
}

/// The named server, as `servers.json` on disk has it right now. `None` once it is gone.
fn saved_server(path: &std::path::Path, name: &str) -> Option<Value> {
    let text = std::fs::read_to_string(path).unwrap_or_default();
    serde_json::from_str::<Value>(&text)
        .ok()?
        .get("servers")?
        .get(name)
        .cloned()
}

/// `iris_remove_server` is refused with the tier off, and the saved server is still in
/// `servers.json` afterwards (FR-025, spec.md Edge Cases).
///
/// The pool is built once at startup and does not hot-reload — the tool's own success note says to
/// restart — so the two halves of this measure different things on purpose. `iris_servers` proves
/// the seed reached the pool *as `iad-native`*, without which `iris_remove_server` refuses with
/// `REMOVE_NOT_ALLOWED` and the tier is never consulted at all. The file on disk is what proves
/// survival.
#[test]
#[ignore]
fn t045_iris_remove_server_refused_and_the_saved_server_survives() {
    if no_iris() {
        return;
    }

    // Declared before the sessions so it outlives them: `Mcp::drop` kills the child, and the
    // tempdir must still exist while it does.
    let home = tempfile::tempdir().expect("home tempdir");
    let servers_json = seeded_servers_path(home.path());
    std::fs::create_dir_all(servers_json.parent().expect("servers.json has a parent"))
        .expect("create the isolated config dir");
    // Written as JSON text rather than a serialized `ServersConfig`, so a serde rename on
    // `ServerEntry` fails here instead of being mirrored by the test (constitution test layer 1).
    std::fs::write(
        &servers_json,
        format!(
            r#"{{"version":1,"servers":{{"{PROBE_SERVER}":{{"host":"127.0.0.1","port":52780,"namespace":"USER","username":"iadgate085"}}}}}}"#
        ),
    )
    .expect("seed servers.json");

    // ── Writes on, tier undeclared ────────────────────────────────────────────
    {
        let mut mcp = Mcp::start_with_home(GATE_ON_WRITES_ONLY, home.path());

        let listed = mcp.call("iris_servers", json!({}));
        let source = listed["servers"]
            .as_array()
            .cloned()
            .unwrap_or_default()
            .into_iter()
            .find(|s| s["name"] == json!(PROBE_SERVER))
            .map(|s| s["source"].clone())
            .unwrap_or(Value::Null);
        assert_eq!(
            source,
            json!("iad-native"),
            "the seeded server must reach the pool as iad-native, or iris_remove_server refuses it \
             for the wrong reason (REMOVE_NOT_ALLOWED) and this test never reaches the tier. \
             HOME was {} and iris_servers answered: {listed}",
            home.path().display()
        );

        let refused = mcp.call("iris_remove_server", json!({"name": PROBE_SERVER}));
        assert_refused(
            &refused,
            ERR_DESTRUCTIVE_GATE,
            "iris_remove_server, tier off",
        );
        assert!(
            saved_server(&servers_json, PROBE_SERVER).is_some(),
            "the refused iris_remove_server rewrote {} anyway — {PROBE_SERVER} is gone from it: {}",
            servers_json.display(),
            std::fs::read_to_string(&servers_json).unwrap_or_default()
        );
    }

    // ── Both tiers on: the removal goes through ───────────────────────────────
    //
    // Without this the refusal above would be satisfied by an entry no configuration can remove —
    // a typo in the seed, a path the binary never reads, a pool that dropped it.
    {
        let mut mcp = Mcp::start_with_home(GATE_ON_BOTH, home.path());
        let removed = mcp.call("iris_remove_server", json!({"name": PROBE_SERVER}));
        assert_eq!(
            removed["removed"],
            json!(true),
            "both tiers on must permit iris_remove_server: {removed}"
        );
        assert!(
            saved_server(&servers_json, PROBE_SERVER).is_none(),
            "the permitted iris_remove_server left {PROBE_SERVER} in {}: {}",
            servers_json.display(),
            std::fs::read_to_string(&servers_json).unwrap_or_default()
        );
    }
}

/// `skill(action = "forget")` is refused with the tier off, and the skill is still installed.
///
/// tasks.md has this one down as local state with no IRIS side effect to observe. It is not:
/// `forget` is `kill ^SKILLS("<name>")` against the skills namespace (`skills_tools.rs`), so the
/// surviving artifact is a global after all, and it is measured two ways — `skill(describe)` returns
/// the seeded description, and the global is still defined.
///
/// Both, because neither is sufficient alone. `describe` reports `success: true` with
/// `description: "\n"` for a skill that has been killed — `$get` of a missing node writes a newline
/// and `skills_tools.rs` tests `raw.is_empty()`, so its `NOT_FOUND` never fires. That is why this
/// compares the description text rather than the success flag, and why the global read-back is here
/// too: a `describe` that stopped reading `^SKILLS` at all would still answer.
#[test]
#[ignore]
fn t045_skill_forget_refused_and_the_skill_survives() {
    if no_iris() {
        return;
    }

    // `description|body|usage_count|created_at` — the layout `describe` splits on.
    let seed_desc = "probe skill for spec 085";
    let seed_value = format!("{seed_desc}|body|0|2026-08-25");

    // ── Writes on, tier undeclared ────────────────────────────────────────────
    {
        let mut mcp = Mcp::start(GATE_ON_WRITES_ONLY);
        let set = mcp.call(
            "iris_global",
            json!({
                "action": "set",
                "global_name": "SKILLS",
                "subscripts": [PROBE_SKILL],
                "value": seed_value,
            }),
        );
        assert_eq!(
            set["success"],
            json!(true),
            "could not seed ^SKILLS(\"{PROBE_SKILL}\") with writes on: {set}"
        );

        // The seeded *description*, not just `success`. Sabotaging the tier showed that `describe`
        // answers `success: true` for a skill that no longer exists, so a bare success check here
        // and below would have been an assertion that cannot fail.
        let described = mcp.call("skill", json!({"action": "describe", "name": PROBE_SKILL}));
        assert_eq!(
            described["description"],
            json!(seed_desc),
            "the seeded skill is not visible to skill(describe), so its survival below would prove \
             nothing: {described}"
        );

        let refused = mcp.call("skill", json!({"action": "forget", "name": PROBE_SKILL}));
        assert_refused(&refused, ERR_DESTRUCTIVE_GATE, "skill forget, tier off");

        let after = mcp.call("skill", json!({"action": "describe", "name": PROBE_SKILL}));
        assert_eq!(
            after["description"],
            json!(seed_desc),
            "the refused skill(forget) removed the skill anyway: {after}"
        );
        let got = mcp.call(
            "iris_global",
            json!({"action": "get", "global_name": "SKILLS", "subscripts": [PROBE_SKILL]}),
        );
        assert_eq!(
            got["defined"],
            json!(true),
            "^SKILLS(\"{PROBE_SKILL}\") is gone, so the refused forget killed it: {got}"
        );
    }

    // ── Both tiers on: the forget goes through, and this is the cleanup ───────
    {
        let mut mcp = Mcp::start(GATE_ON_BOTH);
        let forgotten = mcp.call("skill", json!({"action": "forget", "name": PROBE_SKILL}));
        assert_eq!(
            forgotten["success"],
            json!(true),
            "both tiers on must permit skill(forget): {forgotten}"
        );
        let got = mcp.call(
            "iris_global",
            json!({"action": "get", "global_name": "SKILLS", "subscripts": [PROBE_SKILL]}),
        );
        assert_eq!(
            got["defined"],
            json!(false),
            "the permitted skill(forget) left ^SKILLS(\"{PROBE_SKILL}\") behind: {got}"
        );
    }
}

// ── T046: ordering, and the positive cases ────────────────────────────────────
//
// T046 asks for two things, and both are already asserted above — deliberately not duplicated
// here, because a second copy of an assertion is a second thing to keep true.
//
// Ordering (Destructive is a subset of Write, so writes-off must answer WRITE_TOOLS_DISABLED and
// not DESTRUCTIVE_TOOLS_DISABLED): `t016_every_write_capable_tool_is_refused_with_the_gate_off`
// covers every destructive item there is, not a sample. It walks `CLASSIFICATION` and asserts
// `ERR_WRITE_GATE` for each non-read-only tool and action, which includes the `de(…)` rows
// (`global_kill`, `iris_namespace_create`, `iris_remove_server`, `skill_forget`) and the
// destructive actions of the `mixed(…)` rows. `t018_destructive_tier_gates_the_kill_and_the_global
// _survives` asserts the same order for one row in isolation, with the global surviving both ways.
//
// The positive cases: `t018` (block 3), `t044` (`global_kill` with a fresh token, and the lookup
// delete), and both `t045` tests each end with `GATE_ON_BOTH` and require the operation to go
// through. That is not politeness — without it every refusal above would be satisfied by a tier
// that never opens, which is the vacuous-pass shape this feature exists to remove.
