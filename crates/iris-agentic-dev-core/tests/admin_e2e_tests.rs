#![allow(dead_code, clippy::zombie_processes)]
use std::io::{BufRead, BufReader, Write};
use std::process::{Command, Stdio};

/// Locates the built `iris-agentic-dev` binary (the `[[bin]] name` in
/// `iris-agentic-dev-bin`'s Cargo.toml — NOT `iris-dev`, a stale name from before a
/// crate rename). Checks both `target/{debug,release}/` (plain `cargo build`/`cargo
/// test`) and `target/llvm-cov-target/{debug,release}/` (`cargo llvm-cov`, which
/// builds into a separate target dir) since this test is exercised by both.
fn iris_dev_bin() -> std::path::PathBuf {
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
    // Fall back to the plain debug path so the resulting error message names the
    // path we expected, rather than an empty PathBuf.
    workspace_root.join("target/debug/iris-agentic-dev")
}

fn mcp_exchange(
    messages: &[serde_json::Value],
    extra_env: &[(&str, &str)],
) -> Vec<serde_json::Value> {
    let bin = iris_dev_bin();
    let iris_host = std::env::var("IRIS_HOST").unwrap_or_default();
    let iris_port = std::env::var("IRIS_WEB_PORT").unwrap_or_else(|_| "52780".to_string());

    let mut cmd = Command::new(&bin);
    cmd.args(["mcp"])
        .env("IRIS_HOST", &iris_host)
        .env("IRIS_WEB_PORT", &iris_port)
        .env(
            "IRIS_USERNAME",
            std::env::var("IRIS_USERNAME").unwrap_or_else(|_| "_SYSTEM".into()),
        )
        .env(
            "IRIS_PASSWORD",
            std::env::var("IRIS_PASSWORD").unwrap_or_else(|_| "SYS".into()),
        )
        .env("IRIS_NAMESPACE", "USER")
        .env("IRIS_TOOLSET", "merged");

    for (k, v) in extra_env {
        cmd.env(k, v);
    }

    let mut child = cmd
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
        .expect("spawn iris-dev mcp");

    let mut stdin = child.stdin.take().unwrap();
    let stdout = child.stdout.take().unwrap();
    let mut reader = BufReader::new(stdout);
    let mut results = vec![];

    for msg in messages.iter() {
        stdin
            .write_all((serde_json::to_string(msg).unwrap() + "\n").as_bytes())
            .unwrap();
        stdin.flush().unwrap();
        if msg.get("id").is_some() {
            let deadline = std::time::Instant::now() + std::time::Duration::from_secs(15);
            loop {
                let mut line = String::new();
                std::thread::sleep(std::time::Duration::from_millis(50));
                if reader.read_line(&mut line).unwrap_or(0) > 0 {
                    if let Ok(v) = serde_json::from_str::<serde_json::Value>(&line) {
                        results.push(v);
                        break;
                    }
                }
                if std::time::Instant::now() > deadline {
                    break;
                }
            }
        } else {
            std::thread::sleep(std::time::Duration::from_millis(100));
        }
    }
    child.kill().ok();
    results
}

fn find_response(responses: &[serde_json::Value], id: u64) -> Option<serde_json::Value> {
    responses.iter().find(|r| r["id"] == id).cloned()
}

fn parse_tool_text(response: &serde_json::Value) -> serde_json::Value {
    let text = response["result"]["content"][0]["text"]
        .as_str()
        .unwrap_or("{}");
    serde_json::from_str(text).unwrap_or_default()
}

fn admin_call(action: serde_json::Value, extra_env: &[(&str, &str)]) -> serde_json::Value {
    let responses = mcp_exchange(
        &[
            serde_json::json!({"jsonrpc":"2.0","id":1,"method":"initialize","params":{"protocolVersion":"2024-11-05","capabilities":{},"clientInfo":{"name":"e2e","version":"0.1"}}}),
            serde_json::json!({"jsonrpc":"2.0","method":"notifications/initialized","params":{}}),
            serde_json::json!({"jsonrpc":"2.0","id":2,"method":"tools/call","params":{"name":"iris_admin","arguments":action}}),
        ],
        extra_env,
    );
    find_response(&responses, 2)
        .map(|r| parse_tool_text(&r))
        .unwrap_or_default()
}

fn iris_available() -> bool {
    !std::env::var("IRIS_HOST").unwrap_or_default().is_empty()
}

// ── T016: list_namespaces ─────────────────────────────────────────────────────

#[test]
#[ignore = "requires live IRIS (set IRIS_HOST)"]
fn test_admin_list_namespaces() {
    assert!(iris_available(), "IRIS_HOST must be set");

    let start = std::time::Instant::now();
    let result = admin_call(serde_json::json!({"action": "list_namespaces"}), &[]);
    // T046: SC-004 timing
    assert!(
        start.elapsed().as_secs() < 3,
        "list_namespaces took > 3s (SC-004)"
    );

    assert_eq!(
        result["success"], true,
        "list_namespaces failed: {:?}",
        result
    );
    let namespaces = result["namespaces"]
        .as_array()
        .expect("namespaces must be array");
    let names: Vec<&str> = namespaces
        .iter()
        .filter_map(|n| n["name"].as_str())
        .collect();
    assert!(
        names.contains(&"USER"),
        "USER namespace must be present, got: {:?}",
        names
    );
}

// ── T017: list_databases ──────────────────────────────────────────────────────

#[test]
#[ignore = "requires live IRIS"]
fn test_admin_list_databases() {
    assert!(iris_available());
    let result = admin_call(serde_json::json!({"action": "list_databases"}), &[]);
    assert_eq!(result["success"], true);
    let dbs = result["databases"]
        .as_array()
        .expect("databases must be array");
    assert!(!dbs.is_empty(), "at least one database must be returned");
    // Verify shape
    let db = &dbs[0];
    assert!(db.get("directory").is_some());
    assert!(db.get("size_mb").is_some());
    assert!(db.get("mounted").is_some());
}

// ── T020: list_users, list_user_roles, check_permission ──────────────────────

#[test]
#[ignore = "requires live IRIS"]
fn test_admin_list_users() {
    assert!(iris_available());
    let result = admin_call(serde_json::json!({"action": "list_users"}), &[]);
    assert_eq!(result["success"], true);
    let users = result["users"].as_array().expect("users must be array");
    let names: Vec<&str> = users.iter().filter_map(|u| u["name"].as_str()).collect();
    assert!(
        names.contains(&"_SYSTEM"),
        "_SYSTEM must be in users, got: {:?}",
        names
    );

    // SC-002: no password in response
    let text = result.to_string();
    assert!(
        !text.contains("\"password\""),
        "password must not appear in list_users response"
    );
    assert!(!text.contains("\"Password\""), "Password must not appear");
}

#[test]
#[ignore = "requires live IRIS"]
fn test_admin_list_user_roles() {
    assert!(iris_available());
    let result = admin_call(
        serde_json::json!({"action": "list_user_roles", "username": "_SYSTEM"}),
        &[],
    );
    assert_eq!(
        result["success"], true,
        "list_user_roles failed: {:?}",
        result
    );
    let roles = result["roles"].as_array().expect("roles must be array");
    assert!(!roles.is_empty(), "_SYSTEM must have at least one role");
}

#[test]
#[ignore = "requires live IRIS"]
fn test_admin_check_permission() {
    assert!(iris_available());
    let result = admin_call(
        serde_json::json!({"action": "check_permission", "resource": "%DB_USER", "permission": "USE"}),
        &[],
    );
    assert_eq!(
        result["success"], true,
        "check_permission failed: {:?}",
        result
    );
    assert_eq!(
        result["granted"], true,
        "_SYSTEM should have USE on %DB_USER"
    );
    assert!(result.get("user").is_some(), "user field must be present");
}

// ── T028: list_webapps with and without filter ────────────────────────────────

#[test]
#[ignore = "requires live IRIS"]
fn test_admin_list_webapps() {
    assert!(iris_available());
    let result = admin_call(serde_json::json!({"action": "list_webapps"}), &[]);
    assert_eq!(result["success"], true);
    let webapps = result["webapps"].as_array().expect("webapps must be array");
    let paths: Vec<&str> = webapps.iter().filter_map(|w| w["path"].as_str()).collect();
    assert!(
        paths.iter().any(|p| p.contains("atelier")),
        "/api/atelier must be present, got: {:?}",
        paths
    );

    // Each webapp must have a type field
    for wa in webapps {
        assert!(
            wa.get("type").is_some(),
            "webapp missing type field: {:?}",
            wa
        );
    }
}

#[test]
#[ignore = "requires live IRIS"]
fn test_admin_list_webapps_filter() {
    assert!(iris_available());

    // REST filter — /api/atelier should appear
    let rest_result = admin_call(
        serde_json::json!({"action": "list_webapps", "type": "REST"}),
        &[],
    );
    assert_eq!(rest_result["success"], true);
    let empty_rest = vec![];
    let rest_arr = rest_result["webapps"].as_array().unwrap_or(&empty_rest);
    let rest_paths: Vec<&str> = rest_arr.iter().filter_map(|w| w["path"].as_str()).collect();
    assert!(
        rest_paths.iter().any(|p| p.contains("atelier")),
        "/api/atelier must appear in REST filter"
    );

    // CSP filter — /api/atelier should NOT appear
    let csp_result = admin_call(
        serde_json::json!({"action": "list_webapps", "type": "CSP"}),
        &[],
    );
    assert_eq!(csp_result["success"], true);
    let empty_csp = vec![];
    let csp_arr = csp_result["webapps"].as_array().unwrap_or(&empty_csp);
    let csp_paths: Vec<&str> = csp_arr.iter().filter_map(|w| w["path"].as_str()).collect();
    assert!(
        !csp_paths.iter().any(|p| p.contains("atelier")),
        "/api/atelier must NOT appear in CSP filter"
    );
}

// ── T031: write CRUD round-trips ──────────────────────────────────────────────

#[test]
#[ignore = "requires live IRIS with IRIS_ADMIN_TOOLS=1"]
fn test_admin_user_crud() {
    assert!(iris_available());
    let env = &[("IRIS_ADMIN_TOOLS", "1")];

    // Create
    let r = admin_call(
        serde_json::json!({"action":"create_user","username":"IrisDevTestUser","password":"TestPass123!"}),
        env,
    );
    assert!(
        r["success"] == true || r["error_code"] == "USER_EXISTS",
        "create_user failed: {:?}",
        r
    );

    // List — assert present, no password
    let list = admin_call(serde_json::json!({"action":"list_users"}), env);
    let empty1 = vec![];
    let names: Vec<&str> = list["users"]
        .as_array()
        .unwrap_or(&empty1)
        .iter()
        .filter_map(|u| u["name"].as_str())
        .collect();
    assert!(
        names.contains(&"IrisDevTestUser"),
        "user must appear after create"
    );
    assert!(
        !list.to_string().contains("\"password\""),
        "password must not appear"
    );

    // Delete
    let r2 = admin_call(
        serde_json::json!({"action":"delete_user","username":"IrisDevTestUser"}),
        env,
    );
    assert_eq!(r2["success"], true, "delete_user failed: {:?}", r2);

    // List — assert absent
    let list2 = admin_call(serde_json::json!({"action":"list_users"}), env);
    let empty2 = vec![];
    let names2: Vec<&str> = list2["users"]
        .as_array()
        .unwrap_or(&empty2)
        .iter()
        .filter_map(|u| u["name"].as_str())
        .collect();
    assert!(
        !names2.contains(&"IrisDevTestUser"),
        "user must be gone after delete"
    );
}

#[test]
#[ignore = "requires live IRIS with IRIS_ADMIN_TOOLS=1"]
fn test_admin_namespace_crud() {
    assert!(iris_available());
    let env = &[("IRIS_ADMIN_TOOLS", "1")];

    // Create (reuse USER database for both code and data)
    let r = admin_call(
        serde_json::json!({"action":"create_namespace","name":"IRISDEVTEST","code_database":"USER","data_database":"USER"}),
        env,
    );
    assert!(
        r["success"] == true || r["error_code"] == "NAMESPACE_EXISTS",
        "create_namespace failed: {:?}",
        r
    );

    // List — assert present
    let list = admin_call(serde_json::json!({"action":"list_namespaces"}), &[]);
    let empty3 = vec![];
    let names: Vec<&str> = list["namespaces"]
        .as_array()
        .unwrap_or(&empty3)
        .iter()
        .filter_map(|n| n["name"].as_str())
        .collect();
    assert!(
        names.contains(&"IRISDEVTEST"),
        "namespace must appear after create"
    );

    // Delete
    let r2 = admin_call(
        serde_json::json!({"action":"delete_namespace","name":"IRISDEVTEST"}),
        env,
    );
    assert_eq!(r2["success"], true, "delete_namespace failed: {:?}", r2);

    // List — assert absent
    let list2 = admin_call(serde_json::json!({"action":"list_namespaces"}), &[]);
    let empty4 = vec![];
    let names2: Vec<&str> = list2["namespaces"]
        .as_array()
        .unwrap_or(&empty4)
        .iter()
        .filter_map(|n| n["name"].as_str())
        .collect();
    assert!(
        !names2.contains(&"IRISDEVTEST"),
        "namespace must be gone after delete"
    );
}

#[test]
#[ignore = "requires live IRIS with IRIS_ADMIN_TOOLS=1 AND HealthShare/Ensemble \
support — Security.Applications.Create() internally calls \
%ZHSLIB.HealthShareMgr, which does not exist on IRIS Community edition \
(confirmed via %Dictionary.ClassDefinition.%ExistsId returning 0 on \
intersystemsdc/iris-community:2026.2). This is an IRIS-internal dependency, \
not a bug in admin_create_webapp_impl — the code correctly calls the \
documented Security.Applications API. Fails with \
<CLASS DOES NOT EXIST>IsHealthShareInstalled+3^%Library.EnsembleMgr on \
Community-edition containers; only run this test against an Enterprise/ \
Ensemble-capable IRIS instance."]
fn test_admin_webapp_crud() {
    // Skip on Community edition — requires HealthShare/Ensemble (%ZHSLIB.HealthShareMgr)
    if std::env::var("IRIS_HEALTHSHARE").is_err() {
        return;
    }
    assert!(iris_available());
    let env = &[("IRIS_ADMIN_TOOLS", "1")];

    // Create
    let r = admin_call(
        serde_json::json!({"action":"create_webapp","path":"/irisdevtest","namespace":"USER"}),
        env,
    );
    assert!(
        r["success"] == true || r["error_code"] == "WEBAPP_EXISTS",
        "create_webapp failed: {:?}",
        r
    );

    // List — assert present
    let list = admin_call(serde_json::json!({"action":"list_webapps"}), &[]);
    let empty5 = vec![];
    let paths: Vec<&str> = list["webapps"]
        .as_array()
        .unwrap_or(&empty5)
        .iter()
        .filter_map(|w| w["path"].as_str())
        .collect();
    assert!(
        paths.contains(&"/irisdevtest"),
        "webapp must appear after create"
    );

    // Delete
    let r2 = admin_call(
        serde_json::json!({"action":"delete_webapp","path":"/irisdevtest"}),
        env,
    );
    assert_eq!(r2["success"], true, "delete_webapp failed: {:?}", r2);

    // List — assert absent
    let list2 = admin_call(serde_json::json!({"action":"list_webapps"}), &[]);
    let empty6 = vec![];
    let paths2: Vec<&str> = list2["webapps"]
        .as_array()
        .unwrap_or(&empty6)
        .iter()
        .filter_map(|w| w["path"].as_str())
        .collect();
    assert!(
        !paths2.contains(&"/irisdevtest"),
        "webapp must be gone after delete"
    );
}
