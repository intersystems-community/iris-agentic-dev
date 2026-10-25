//! Spec 141 R2 guards: no execution tool picks its own connection (T-R2-U12), and `check_config`
//! reports the identity each path would use (T-R2-U13).

use iris_agentic_dev_core::iris::config_sources::{ConfigSnapshot, LoadInputs, CONFIG_FILE_NAME};
use iris_agentic_dev_core::tools::exec::{
    identity_block, ExecRoute, MemberIdentity, SERVICE_ACCOUNT_TOOLS,
};
use serde_json::json;

// ── U12: source scan ──────────────────────────────────────────────────────────

/// The `#[tool]` function bodies of `tools/mod.rs`, by name.
fn tool_bodies(src: &str) -> Vec<(String, String)> {
    let mut out = Vec::new();
    let mut rest = src;
    while let Some(at) = rest.find("#[tool(") {
        rest = &rest[at + 7..];
        let Some(f) = rest.find("async fn ") else {
            break;
        };
        let after = &rest[f + 9..];
        let name: String = after
            .chars()
            .take_while(|c| c.is_alphanumeric() || *c == '_')
            .collect();
        // The body ends where the next `#[tool(` begins, or at the end of the file.
        let end = after.find("#[tool(").unwrap_or(after.len());
        out.push((name, after[..end].to_string()));
    }
    out
}

const FORBIDDEN: &[&str] = &[
    "with_service_account(",
    "exec_client",
    ".execute(",
    ".execute_ssh(",
    "container_from_env(",
];

#[test]
fn no_execution_tool_body_picks_its_own_connection() {
    let src = std::fs::read_to_string(
        std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("src/tools/mod.rs"),
    )
    .unwrap();
    let bodies = tool_bodies(&src);
    assert!(
        bodies.len() > 50,
        "the scan found {} tool bodies; it would pass for free",
        bodies.len()
    );
    let mut checked = 0;
    let mut offenders = Vec::new();
    for (name, body) in &bodies {
        if !SERVICE_ACCOUNT_TOOLS.contains(&name.as_str()) {
            continue;
        }
        checked += 1;
        for pat in FORBIDDEN {
            if body.contains(pat) {
                offenders.push(format!("{name}: {pat}"));
            }
        }
    }
    assert!(checked >= 9, "only {checked} agent-code tools were scanned");
    assert!(
        offenders.is_empty(),
        "a tool that runs the agent's code must go through exec::run_code: {offenders:?}"
    );
}

#[test]
fn the_exec_client_field_is_gone() {
    let src = std::fs::read_to_string(
        std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("src/tools/mod.rs"),
    )
    .unwrap();
    assert!(
        !src.contains("pub exec_client:") && !src.contains("fn get_iris_for_exec_with_client"),
        "the shared exec_client and its resolver are replaced by per-identity clients"
    );
}

// ── U13: the check_config identity block ──────────────────────────────────────

fn snapshot(home: &str, project: Option<&str>) -> (ConfigSnapshot, tempfile::TempDir) {
    let dir = tempfile::tempdir().unwrap();
    let root = std::fs::canonicalize(dir.path()).unwrap();
    let h = root.join("home");
    let p = root.join("proj");
    std::fs::create_dir_all(&h).unwrap();
    std::fs::create_dir_all(&p).unwrap();
    std::fs::write(h.join(CONFIG_FILE_NAME), home).unwrap();
    if let Some(body) = project {
        std::fs::write(p.join(CONFIG_FILE_NAME), body).unwrap();
    }
    let snap = ConfigSnapshot::load(&LoadInputs {
        cwd: p,
        home: Some(h),
        ..Default::default()
    });
    (snap, dir)
}

#[test]
fn the_identity_block_reports_account_opt_in_paths_servers_and_tools() {
    let (snap, _d) = snapshot("", Some("exec_as_os_user = true\n"));
    let members = vec![
        MemberIdentity {
            name: "_env".into(),
            decision: json!({"user": "IADSVC", "source": "service_account_env"}),
            route: ExecRoute::Http,
        },
        MemberIdentity {
            name: "qa".into(),
            decision: json!({"refused": "SERVICE_ACCOUNT_UNCONFIGURED_FOR_SERVER"}),
            route: ExecRoute::Http,
        },
    ];
    let route = ExecRoute::HttpThenDocker {
        container: "iris-dev-iris".into(),
        ssh_host: None,
    };
    let block = identity_block(&snap, Some("IADSVC"), None, &route, &members);

    assert_eq!(block["service_account"]["configured"], json!(true));
    assert_eq!(block["service_account"]["user"], json!("IADSVC"));
    assert_eq!(
        block["service_account"]["source"],
        json!("service_account_env")
    );

    assert_eq!(block["os_user_opt_in"]["enabled"], json!(false));
    assert!(
        block["os_user_opt_in"]["ignored"]
            .as_array()
            .is_some_and(|a| a
                .iter()
                .any(|i| i["key"].as_str() == Some("exec_as_os_user"))),
        "{block}"
    );

    assert_eq!(block["paths"]["http"]["user"], json!("IADSVC"));
    assert_eq!(
        block["paths"]["docker_local"]["container"],
        json!("iris-dev-iris")
    );
    assert_eq!(
        block["paths"]["docker_local"]["via"],
        json!("Security.Login guard")
    );
    assert!(block["paths"]["docker_ssh"].is_null());
    assert_eq!(block["paths"]["ws_terminal"]["user"], json!("IADSVC"));

    let servers = block["servers"].as_array().unwrap();
    assert_eq!(servers.len(), 2);
    assert_eq!(servers[1]["name"], json!("qa"));
    assert_eq!(
        servers[1]["refused"],
        json!("SERVICE_ACCOUNT_UNCONFIGURED_FOR_SERVER")
    );
    assert_eq!(servers[1]["route"], json!("http"));

    let tools: Vec<&str> = block["tools"]
        .as_array()
        .unwrap()
        .iter()
        .map(|t| t.as_str().unwrap())
        .collect();
    for t in SERVICE_ACCOUNT_TOOLS {
        assert!(tools.contains(t), "{t} missing from {tools:?}");
    }
}

#[test]
fn the_identity_block_with_no_account_says_so_and_names_the_opt_in_source() {
    let (snap, _d) = snapshot("exec_as_os_user = true\n", None);
    let route = ExecRoute::DockerOnly {
        container: "c".into(),
        ssh_host: Some("h".into()),
    };
    let block = identity_block(&snap, None, Some("1"), &route, &[]);
    assert_eq!(block["service_account"]["configured"], json!(false));
    assert_eq!(block["os_user_opt_in"]["enabled"], json!(true));
    assert_eq!(block["os_user_opt_in"]["source"], json!("env"));
    assert!(
        block["paths"]["http"].is_null(),
        "docker_only has no HTTP leg"
    );
    assert_eq!(block["paths"]["docker_ssh"]["host"], json!("h"));
}

#[test]
fn every_service_account_tool_is_a_registered_tool() {
    let tools = iris_agentic_dev_core::tools::IrisTools::new(None).unwrap();
    let registered = tools.registered_tool_names();
    for t in SERVICE_ACCOUNT_TOOLS {
        assert!(
            registered.iter().any(|r| r == t),
            "{t} is in SERVICE_ACCOUNT_TOOLS and is not a registered tool; the identity would never \
             be resolved for it"
        );
    }
}
