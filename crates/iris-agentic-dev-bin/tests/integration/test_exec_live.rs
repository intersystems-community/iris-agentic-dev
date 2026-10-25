use std::process::Command;

fn env_or(key: &str, default: &str) -> String {
    std::env::var(key).unwrap_or_else(|_| default.to_string())
}

fn iris_dev() -> Command {
    let bin = env!("CARGO_BIN_EXE_iris-agentic-dev");
    let mut cmd = Command::new(bin);
    crate::support_141::isolate(&mut cmd);
    cmd.env("IRIS_HOST", env_or("IRIS_HOST", "localhost"))
        .env("IRIS_WEB_PORT", env_or("IRIS_WEB_PORT", "52780"))
        .env("IRIS_NAMESPACE", env_or("IRIS_NAMESPACE", "USER"))
        .env("IRIS_USERNAME", env_or("IRIS_USERNAME", "_SYSTEM"))
        .env("IRIS_PASSWORD", env_or("IRIS_PASSWORD", "SYS"))
        .env("IRIS_ALLOW_PROD", "1");
    cmd
}

#[test]
#[ignore]
fn test_exec_zversion() {
    let out = iris_dev()
        .args(["exec", "write $ZVersion,!"])
        .output()
        .expect("failed to run iris-agentic-dev");
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(
        out.status.success(),
        "expected exit 0, got {:?}\nstdout: {}",
        out.status,
        stdout
    );
    assert!(!stdout.trim().is_empty(), "expected non-empty output");
    assert!(
        stdout.contains("IRIS"),
        "expected version string to contain 'IRIS', got: {}",
        stdout
    );
}

#[test]
#[ignore]
fn test_exec_macro_ok() {
    let out = iris_dev()
        .args(["exec", "write $$$OK,!"])
        .output()
        .expect("failed to run iris-agentic-dev");
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(out.status.success(), "expected exit 0\nstdout: {}", stdout);
    assert_eq!(stdout.trim(), "1", "expected $$$OK=1, got: {}", stdout);
}

#[test]
#[ignore]
fn test_exec_file() {
    let tmp = tempfile::NamedTempFile::with_suffix(".cos").unwrap();
    std::fs::write(tmp.path(), "write \"hello-from-file\",!\n").unwrap();
    let out = iris_dev()
        .args(["exec", "--file", tmp.path().to_str().unwrap()])
        .output()
        .expect("failed to run iris-agentic-dev");
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(out.status.success(), "expected exit 0\nstdout: {}", stdout);
    assert!(
        stdout.contains("hello-from-file"),
        "expected output, got: {}",
        stdout
    );
}

#[test]
#[ignore]
fn test_exec_runtime_error_in_output() {
    // IRIS runtime errors are reported in stdout (the HTTP generator returns 200 with error text).
    // The binary exits 0 but the error is visible — callers should inspect stdout for ERROR:.
    let out = iris_dev()
        .args(["exec", "do ##class(Nonexistent.Class).Method()"])
        .output()
        .expect("failed to run iris-agentic-dev");
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(
        !stdout.is_empty(),
        "expected error text in stdout for IRIS runtime error"
    );
    assert!(
        stdout.contains("CLASS DOES NOT EXIST") || stdout.contains("ERROR"),
        "expected IRIS error text in stdout, got: {}",
        stdout
    );
}

#[test]
#[ignore]
fn test_exec_namespace_flag() {
    let out = iris_dev()
        .args(["exec", "--namespace", "USER", "write $namespace,!"])
        .output()
        .expect("failed to run iris-agentic-dev");
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(out.status.success(), "expected exit 0\nstdout: {}", stdout);
    assert!(
        stdout.trim().eq_ignore_ascii_case("user"),
        "expected namespace USER, got: {}",
        stdout
    );
}

/// IRIS must be able to tell that a call came from this tool rather than from a developer's
/// IDE. The only place that shows up is the `User-Agent` header, which the Web Gateway (or
/// IIS/Apache) writes to its access log and which ObjectScript can read from
/// `%request.CgiEnvs`. Asserting it end to end is the point: a unit test on the string
/// cannot catch a client that was built without the header attached.
#[test]
#[ignore]
fn test_user_agent_visible_to_iris() {
    let out = iris_dev()
        .env("IRIS_AGENT_LABEL", "live-test-label")
        .args([
            "exec",
            "write $Get(%request.CgiEnvs(\"HTTP_USER_AGENT\"),\"<none>\"),!",
        ])
        .output()
        .expect("failed to run iris-agentic-dev");
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(out.status.success(), "expected exit 0\nstdout: {}", stdout);
    let ua = stdout.trim();
    assert!(
        ua.starts_with("iris-agentic-dev/"),
        "IRIS saw User-Agent {:?}; an operator filtering agent traffic out of a production \
         environment has nothing to filter on unless this is set",
        ua
    );
    assert!(
        ua.contains("cli"),
        "expected the one-shot CLI caller mode in {:?}",
        ua
    );
    assert!(
        ua.contains("live-test-label"),
        "expected IRIS_AGENT_LABEL to reach IRIS in {:?}",
        ua
    );
}

// ── Spec 087: iris_execute destructive gate ───────────────────────────────────

/// T087-binary: `Kill ^global` in the code string is refused when the destructive gate is off.
/// No live IRIS required — the gate fires before any IRIS network call.
#[test]
#[ignore]
fn test_exec_kill_global_blocked_when_destructive_gate_off() {
    let out = iris_dev()
        .args(["exec", "Kill ^IadGateTest"])
        // Write gate on, destructive gate off.
        .env("IRIS_WRITE_TOOLS_ENABLED", "1")
        .env("IRIS_DESTRUCTIVE_TOOLS_ENABLED", "0")
        // Point at a non-existent host — the gate check must fire before any IRIS call.
        .env("IRIS_HOST", "127.0.0.1")
        .env("IRIS_WEB_PORT", "19999")
        .output()
        .expect("failed to run iris-agentic-dev");
    let stdout = String::from_utf8_lossy(&out.stdout);
    let stderr = String::from_utf8_lossy(&out.stderr);
    let combined = format!("{stdout}{stderr}");
    // The CLI prints the error message text, not the error_code field. Check for the
    // distinctive phrase that appears in the 087 destructive-gate refusal message.
    assert!(
        combined.contains("destructive tier is disabled"),
        "expected destructive-gate refusal for Kill ^IadGateTest with destructive gate off, got:\n\
         stdout: {stdout}\nstderr: {stderr}"
    );
}

/// T087-binary-pass: with both gates on, `Kill ^<global>` is not refused by the gate
/// (it may fail for other reasons without live IRIS, which is fine — no DESTRUCTIVE_TOOLS_DISABLED).
#[test]
#[ignore]
fn test_exec_kill_global_not_blocked_when_destructive_gate_on() {
    let out = iris_dev()
        .args(["exec", "Kill ^IadGateTest"])
        .env("IRIS_WRITE_TOOLS_ENABLED", "1")
        .env("IRIS_DESTRUCTIVE_TOOLS_ENABLED", "1")
        .env("IRIS_HOST", "127.0.0.1")
        .env("IRIS_WEB_PORT", "19999")
        .output()
        .expect("failed to run iris-agentic-dev");
    let stdout = String::from_utf8_lossy(&out.stdout);
    let stderr = String::from_utf8_lossy(&out.stderr);
    let combined = format!("{stdout}{stderr}");
    assert!(
        !combined.contains("destructive tier is disabled"),
        "destructive-gate refusal must not fire when the destructive gate is on:\n\
         stdout: {stdout}\nstderr: {stderr}"
    );
}

/// T087-live-block: with live IRIS, `Kill ^IadGateTest` with destructive gate off returns the
/// refusal before IRIS is called.
#[test]
#[ignore]
fn test_exec_kill_global_blocked_live() {
    let out = iris_dev()
        .args(["exec", "Kill ^IadGateTest"])
        .env("IRIS_WRITE_TOOLS_ENABLED", "1")
        .env("IRIS_DESTRUCTIVE_TOOLS_ENABLED", "0")
        .output()
        .expect("failed to run iris-agentic-dev");
    let stdout = String::from_utf8_lossy(&out.stdout);
    let stderr = String::from_utf8_lossy(&out.stderr);
    let combined = format!("{stdout}{stderr}");
    assert!(
        combined.contains("destructive tier is disabled"),
        "expected destructive-gate refusal with live IRIS and destructive gate off:\n\
         stdout: {stdout}\nstderr: {stderr}"
    );
}

/// T087-live-allow: with live IRIS and both gates on, `Kill ^IadGateTest` succeeds (or errors
/// for legitimate IRIS reasons — but NOT DESTRUCTIVE_TOOLS_DISABLED).
/// Sets the global first so the kill has something to remove.
#[test]
#[ignore]
fn test_exec_kill_global_allowed_live() {
    // Set up: create the global node so the kill is meaningful.
    let setup = iris_dev()
        .args(["exec", "Set ^IadGateTest=\"iad-087-probe\""])
        .env("IRIS_WRITE_TOOLS_ENABLED", "1")
        .env("IRIS_DESTRUCTIVE_TOOLS_ENABLED", "1")
        .output()
        .expect("failed to run iris-agentic-dev (setup)");
    assert!(
        setup.status.success(),
        "setup Set ^IadGateTest failed: {}",
        String::from_utf8_lossy(&setup.stdout)
    );

    // Exercise: kill it with the destructive gate on.
    let out = iris_dev()
        .args(["exec", "Kill ^IadGateTest"])
        .env("IRIS_WRITE_TOOLS_ENABLED", "1")
        .env("IRIS_DESTRUCTIVE_TOOLS_ENABLED", "1")
        .output()
        .expect("failed to run iris-agentic-dev");
    let stdout = String::from_utf8_lossy(&out.stdout);
    let stderr = String::from_utf8_lossy(&out.stderr);
    let combined = format!("{stdout}{stderr}");
    assert!(
        !combined.contains("DESTRUCTIVE_TOOLS_DISABLED"),
        "DESTRUCTIVE_TOOLS_DISABLED must not fire when both gates are on:\n\
         stdout: {stdout}\nstderr: {stderr}"
    );

    // Verify: global is gone.
    let verify = iris_dev()
        .args(["exec", "Write $Data(^IadGateTest)"])
        .output()
        .expect("failed to run iris-agentic-dev (verify)");
    let vstdout = String::from_utf8_lossy(&verify.stdout);
    assert!(
        vstdout.trim() == "0",
        "^IadGateTest should be gone after Kill, $Data returned: {vstdout}"
    );
}

// --- 130 round 4: a CLI call leaves no telemetry scratch class behind ---------------------------
//
// With the operator's opt-in (`IAD_TELEMETRY_SINK=iris`), every tool call on a server with writes
// on writes a telemetry record to `^IRISDEV("telemetry",...)` through a scratch class.
// `record_call` spawned that write and the CLI exited without waiting, so the runtime dropped it
// between compile and delete: 78,183 `IrisDevTmp.IrisDevRun*` classes in USER. Telemetry now uses
// its own prefix, so this counts the fix alone and not the older MCP processes still running.

fn telemetry_scratch_classes() -> std::collections::BTreeSet<String> {
    let url = format!(
        "http://{}:{}/api/atelier/v1/USER/docnames/CLS?filter=IrisDevTmp.IrisDevTel%25",
        env_or("IRIS_HOST", "localhost"),
        env_or("IRIS_WEB_PORT", "52780")
    );
    let auth = format!(
        "{}:{}",
        env_or("IRIS_USERNAME", "_SYSTEM"),
        env_or("IRIS_PASSWORD", "SYS")
    );
    let out = Command::new("curl")
        .args(["-s", "-u", &auth, &url])
        .output()
        .expect("curl");
    let body: serde_json::Value = serde_json::from_slice(&out.stdout)
        .unwrap_or_else(|e| panic!("docnames answer is not JSON ({e}): {:?}", out));
    body["result"]["content"]
        .as_array()
        .expect("docnames content")
        .iter()
        .filter_map(|d| d["name"].as_str())
        .filter(|n| n.contains("IrisDevTel"))
        .map(str::to_string)
        .collect()
}

fn telemetry_sessions() -> u64 {
    let out = iris_dev()
        .args([
            "exec",
            r#"set n=0,k="" for  { set k=$order(^IRISDEV("telemetry",k)) quit:k=""  set n=n+1 } write n,!"#,
        ])
        .output()
        .expect("failed to run iris-agentic-dev");
    String::from_utf8_lossy(&out.stdout)
        .trim()
        .parse()
        .unwrap_or_else(|_| panic!("session count: {:?}", out))
}

#[test]
#[ignore]
fn test_exec_leaves_no_telemetry_scratch_class_130() {
    let before = telemetry_sessions();
    let iris_sink = || {
        let mut cmd = iris_dev();
        cmd.env("IAD_TELEMETRY_SINK", "iris");
        cmd
    };
    // Names, not a count: a class another suite leaked (a killed server, a dropped runtime) must
    // not fail this one, and one this test leaks must not hide behind a cleanup elsewhere.
    let classes_before = telemetry_scratch_classes();

    let ok = iris_sink().args(["exec", "write 1,!"]).output().unwrap();
    assert!(ok.status.success(), "{ok:?}");
    // A failing call exits through the other path, `exit(1)`.
    let failed = iris_sink()
        .args(["exec", "do ##class(Nonexistent.Class130).Method()"])
        .output()
        .unwrap();
    assert!(
        !failed.status.success() || !failed.stdout.is_empty(),
        "{failed:?}"
    );

    let leaked: Vec<_> = telemetry_scratch_classes()
        .difference(&classes_before)
        .cloned()
        .collect();
    assert!(
        leaked.is_empty(),
        "a CLI process exited before its telemetry write removed its scratch class: {leaked:?}"
    );
    // The writes happened, so no new class means cleaned up and not skipped. The counting calls are
    // CLI processes too, so the number only goes up.
    assert!(
        telemetry_sessions() >= before + 2,
        "no telemetry write landed, so the empty diff above proves nothing"
    );
}
