//! `skill_list` reads the synthesized skills in `^SKILLS` over Atelier, so it finds them with no
//! docker container. It used to read them through a docker terminal only, and CI (IRIS 2025.3)
//! reported `synthesized.searched=false` although the test had just written the entry.
//!
//! Run: IRIS_HOST=localhost IRIS_WEB_PORT=52780 \
//!   cargo test --features testing --test integration test_skill_list_atelier_live -- --test-threads=1 --include-ignored

use iris_agentic_dev_core::iris::connection::{DiscoverySource, IrisConnection};
use iris_agentic_dev_core::testing::EnvGuard;
use iris_agentic_dev_core::tools::{IrisTools, Toolset};

fn tools() -> IrisTools {
    let host = std::env::var("IRIS_HOST").expect("IRIS_HOST must name the live IRIS for this test");
    let port = std::env::var("IRIS_WEB_PORT").unwrap_or_else(|_| "52780".to_string());
    let user = std::env::var("IRIS_USERNAME").unwrap_or_else(|_| "_SYSTEM".to_string());
    let pass = std::env::var("IRIS_PASSWORD").unwrap_or_else(|_| "SYS".to_string());
    let conn = IrisConnection::new(
        format!("http://{host}:{port}"),
        "USER",
        user,
        pass,
        DiscoverySource::EnvVar,
    );
    IrisTools::for_test_gates_open(Some(conn), Toolset::Merged).unwrap()
}

fn json(r: Result<rmcp::model::CallToolResult, String>) -> serde_json::Value {
    let text = r.unwrap().content[0].as_text().unwrap().text.clone();
    serde_json::from_str(&text).unwrap()
}

#[tokio::test]
#[ignore]
async fn skill_list_finds_a_synthesized_skill_without_docker() {
    let _env = EnvGuard::without("IRIS_CONTAINER");
    let t = tools();
    let name = "iad-atelier-synth-test";
    let set = json(
        t.call_gated(
            "iris_global",
            serde_json::json!({"action": "set", "global_name": "^SKILLS", "subscripts": [name],
                "value": "atelier synthesized skill|body text|0|2026-01-01T00:00:00Z",
                "namespace": "USER"}),
        )
        .await,
    );
    let list = json(t.call_gated("skill_list", serde_json::json!({})).await);
    let described = json(
        t.call_gated("skill_describe", serde_json::json!({"name": name}))
            .await,
    );
    let kill = json(
        t.call_gated(
            "iris_global",
            serde_json::json!({"action": "kill", "global_name": "^SKILLS", "subscripts": [name],
                "namespace": "USER"}),
        )
        .await,
    );
    // Clean up before asserting, so a failing run leaves nothing in ^SKILLS.
    assert_eq!(set["success"], true, "seeding ^SKILLS: {set}");
    assert_eq!(kill["success"], true, "cleanup ^SKILLS: {kill}");
    assert_eq!(list["sources"]["synthesized"]["searched"], true, "{list}");
    let found = list["skills"]
        .as_array()
        .expect("skills array")
        .iter()
        .find(|s| s["name"] == name)
        .unwrap_or_else(|| panic!("{name} not in skill_list: {list}"));
    assert_eq!(found["source"], "synthesized");
    assert_eq!(found["description"], "atelier synthesized skill");
    assert_eq!(described["success"], true, "{described}");
    assert_eq!(described["skill"]["body"], "body text", "{described}");
}

/// The terminal fallback, for a server with no web server, finds the same entry through docker.
#[tokio::test]
#[ignore]
async fn the_terminal_fallback_reads_skills_through_docker() {
    let container = iris_agentic_dev_core::testing::live_container();
    let t = tools();
    let name = "iad-terminal-synth-test";
    let set = json(
        t.call_gated(
            "iris_global",
            serde_json::json!({"action": "set", "global_name": "^SKILLS", "subscripts": [name],
                "value": "terminal synthesized|tbody|0|t", "namespace": "USER"}),
        )
        .await,
    );
    let mut conn = IrisConnection::new(
        format!(
            "http://{}:{}",
            std::env::var("IRIS_HOST").unwrap(),
            std::env::var("IRIS_WEB_PORT").unwrap_or_else(|_| "52780".to_string())
        ),
        "USER",
        "_SYSTEM",
        "SYS",
        DiscoverySource::EnvVar,
    );
    conn.member.container = Some(container);
    let out = conn
        .execute(
            iris_agentic_dev_core::tools::skills_tools::synthesized_skills_terminal_code(),
            "USER",
        )
        .await;
    let kill = json(
        t.call_gated(
            "iris_global",
            serde_json::json!({"action": "kill", "global_name": "^SKILLS", "subscripts": [name],
                "namespace": "USER"}),
        )
        .await,
    );
    assert_eq!(set["success"], true, "{set}");
    assert_eq!(kill["success"], true, "{kill}");
    let out = out.expect("docker terminal run");
    let entries: Vec<serde_json::Value> =
        serde_json::from_str(out.trim()).unwrap_or_else(|e| panic!("not JSON ({e}): {out}"));
    let found = entries
        .iter()
        .find(|e| e["name"] == name)
        .unwrap_or_else(|| panic!("{name} missing: {out}"));
    assert_eq!(found["description"], "terminal synthesized");
    assert_eq!(found["body"], "tbody");
}
