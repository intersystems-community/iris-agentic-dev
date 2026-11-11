//! R2c, against `iris-dev-iris`: `skill` list and search and `kb` recall read the `^SKILLS` and
//! `^KBCHUNKS` globals and answer with JSON.
//!
//! The ObjectScript behind them escaped quotes with a backslash (`\"`). ObjectScript has one
//! escape, a doubled quote, so the generated class did not compile and every call on an HTTP
//! connection failed with `IRIS_EXECUTE_ERROR` after the scratch class was stored. The entries
//! below hold quotes, a backslash, braces and a line break, which any hand-built JSON gets wrong.
//!
//! Serial, `#[ignore]`: run with
//! `lockf -k /tmp/iad-141-iris.lock cargo test --features testing --test integration skills_store_171 -- --ignored --test-threads=1`.

use iris_agentic_dev_core::iris::config_sources::{ConfigSnapshot, LoadInputs, CONFIG_FILE_NAME};
use iris_agentic_dev_core::iris::connection::{DiscoverySource, IrisConnection};
use iris_agentic_dev_core::objectscript::os_str_expr;
use iris_agentic_dev_core::skills::SkillRegistry;
use iris_agentic_dev_core::testing::{LiveTarget, ThrowawayIris};
use iris_agentic_dev_core::tools::{IrisTools, Toolset};
use serde_json::{json, Value};

struct Rig {
    tools: IrisTools,
    conn: IrisConnection,
    client: reqwest::Client,
    _dir: tempfile::TempDir,
}

/// Where the scenarios run: `iris-dev-iris`, or the throwaway container (the default image and
/// `IAD_THROWAWAY_IMAGE`, so the same scenarios run on 2025.3).
struct Where {
    base_url: String,
    user: String,
    password: String,
    namespace: String,
}

impl Where {
    fn live() -> Self {
        let t = LiveTarget::from_env();
        t.assert_running();
        Where {
            base_url: t.base_url(),
            user: t.username.clone(),
            password: t.password.clone(),
            namespace: t.namespace.clone(),
        }
    }

    fn throwaway(t: &ThrowawayIris) -> Self {
        Where {
            base_url: format!("http://{}:{}", t.host, t.web_port),
            user: t.username.to_string(),
            password: t.password.to_string(),
            namespace: "USER".to_string(),
        }
    }
}

async fn rig(at: &Where) -> Rig {
    let dir = tempfile::tempdir().unwrap();
    let root = std::fs::canonicalize(dir.path()).unwrap();
    let home = root.join("home");
    let proj = root.join("proj");
    std::fs::create_dir_all(&home).unwrap();
    std::fs::create_dir_all(&proj).unwrap();
    std::fs::write(
        home.join(CONFIG_FILE_NAME),
        "write_tools_enabled = true\ndestructive_tools_enabled = true\n",
    )
    .unwrap();
    let inputs = LoadInputs {
        cwd: proj,
        home: Some(home),
        ..Default::default()
    };
    let snapshot = ConfigSnapshot::load(&inputs);
    let declared = snapshot.declared_gates();
    let make = || {
        IrisConnection::new(
            at.base_url.clone(),
            &at.namespace,
            &at.user,
            &at.password,
            DiscoverySource::ExplicitFlag,
        )
    };
    let mut conn = make();
    conn.probe().await;
    let tools = IrisTools::with_registry_and_toolset(
        Some(conn.clone()),
        SkillRegistry::new(),
        Toolset::Baseline,
        None,
        None,
        false,
        declared,
    )
    .unwrap();
    tools.set_config(inputs, snapshot);
    Rig {
        tools,
        conn,
        client: iris_agentic_dev_core::testing::iris_client(&at.user, &at.password),
        _dir: dir,
    }
}

impl Rig {
    async fn run(&self, code: &str) -> String {
        self.conn
            .execute_via_generator(code, "USER", &self.client)
            .await
            .unwrap()
    }

    async fn call(&self, tool: &str, args: Value) -> Value {
        match self.tools.call_gated(tool, args).await {
            Ok(res) => res
                .structured_content
                .clone()
                .or_else(|| {
                    res.content
                        .iter()
                        .find_map(|c| c.as_text())
                        .and_then(|t| serde_json::from_str(&t.text).ok())
                })
                .unwrap_or(json!({})),
            Err(e) => json!({"error": e}),
        }
    }
}

/// Names and text chosen to break hand-built JSON and hand-escaped ObjectScript.
const SKILL_NAME: &str = "r2c-171 \"quoted\" \\ name";
const SKILL_DESC: &str = "has \"quotes\", a \\ backslash and {braces}";
const KB_FILE: &str = "r2c-171-\"kb\".md";
const KB_TEXT: &str = "r2c171needle with \"quotes\" and a \\ backslash {x}";

async fn seed(r: &Rig) {
    let set_skill = format!(
        "Set ^SKILLS({})={}",
        os_str_expr(SKILL_NAME),
        os_str_expr(&format!("{SKILL_DESC}|the body|3|2026-01-01"))
    );
    let set_kb = format!(
        "Set ^KBCHUNKS({})={}",
        os_str_expr(KB_FILE),
        os_str_expr(KB_TEXT)
    );
    r.run(&format!("{set_skill}\n{set_kb}\nWrite \"seeded\""))
        .await;
}

async fn cleanup(r: &Rig) {
    let code = format!(
        "Kill ^SKILLS({})\nKill ^KBCHUNKS({})\nWrite \"clean\"",
        os_str_expr(SKILL_NAME),
        os_str_expr(KB_FILE)
    );
    r.run(&code).await;
}

async fn skill_list_returns_json_for_names_with_quotes_and_backslashes(r: &Rig) {
    seed(r).await;
    let b = r.call("skill", json!({"action": "list"})).await;
    cleanup(r).await;
    assert_eq!(b["success"], true, "{b}");
    let mine = b["skills"]
        .as_array()
        .expect("skills is an array")
        .iter()
        .find(|s| s["name"] == SKILL_NAME)
        .unwrap_or_else(|| panic!("the seeded skill is not listed: {b}"));
    assert_eq!(mine["description"], SKILL_DESC, "{mine}");
    assert_eq!(mine["usage_count"], 3, "{mine}");
}

async fn skill_search_finds_it_by_a_quoted_term(r: &Rig) {
    seed(r).await;
    // The needle holds a quote and a backslash, so it is spliced as an expression, not a literal.
    let by_quote = r
        .call(
            "skill",
            json!({"action": "search", "query": "\"quoted\" \\"}),
        )
        .await;
    let by_word = r
        .call("skill", json!({"action": "search", "query": "r2c-171"}))
        .await;
    let none = r
        .call(
            "skill",
            json!({"action": "search", "query": "zz-no-such-skill-171"}),
        )
        .await;
    cleanup(r).await;
    for (what, b) in [("quote", &by_quote), ("word", &by_word)] {
        assert_eq!(b["success"], true, "{what}: {b}");
        assert!(
            b["results"]
                .as_array()
                .unwrap()
                .iter()
                .any(|s| s["name"] == SKILL_NAME && s["description"] == SKILL_DESC),
            "{what}: {b}"
        );
    }
    assert_eq!(none["success"], true, "{none}");
    assert_eq!(none["results"], json!([]), "{none}");
}

async fn kb_recall_returns_json_for_text_with_quotes_and_backslashes(r: &Rig) {
    seed(r).await;
    let b = r
        .call(
            "kb",
            json!({"action": "recall", "query": "r2c171needle", "top_k": 5}),
        )
        .await;
    let quoted = r
        .call(
            "kb",
            json!({"action": "recall", "query": "\"quotes\" and a \\", "top_k": 5}),
        )
        .await;
    cleanup(r).await;
    for (what, b) in [("word", &b), ("quote", &quoted)] {
        assert_eq!(b["success"], true, "{what}: {b}");
        let hit = b["results"]
            .as_array()
            .unwrap()
            .iter()
            .find(|h| h["file"] == KB_FILE)
            .unwrap_or_else(|| panic!("{what}: the seeded chunk is not recalled: {b}"));
        assert_eq!(hit["excerpt"], KB_TEXT, "{what}: {hit}");
    }
}

async fn an_empty_store_lists_as_an_empty_array(r: &Rig) {
    // Nothing of ours seeded, nothing of ours listed, and the answer is still an array.
    cleanup(r).await;
    let b = r.call("skill", json!({"action": "list"})).await;
    assert_eq!(b["success"], true, "{b}");
    assert!(b["skills"].as_array().is_some(), "{b}");
    assert!(!b["skills"]
        .as_array()
        .unwrap()
        .iter()
        .any(|s| s["name"] == SKILL_NAME));
}

async fn kb_recall_stops_at_top_k_and_at_the_end_of_the_global(r: &Rig) {
    let keys = ["r2c-171-top-a", "r2c-171-top-b", "r2c-171-top-c"];
    let seed: String = keys
        .iter()
        .map(|k| {
            format!(
                "Set ^KBCHUNKS({})={}\n",
                os_str_expr(k),
                os_str_expr("r2c171topk text")
            )
        })
        .collect();
    r.run(&format!("{seed}Write \"seeded\"")).await;
    let two = r
        .call(
            "kb",
            json!({"action": "recall", "query": "r2c171topk", "top_k": 2}),
        )
        .await;
    // More than there are: the loop has to end at the last key, not read past it.
    let all = r
        .call(
            "kb",
            json!({"action": "recall", "query": "r2c171topk", "top_k": 50}),
        )
        .await;
    let kill: String = keys
        .iter()
        .map(|k| format!("Kill ^KBCHUNKS({})\n", os_str_expr(k)))
        .collect();
    r.run(&format!("{kill}Write \"clean\"")).await;
    assert_eq!(two["success"], true, "{two}");
    assert_eq!(two["results"].as_array().unwrap().len(), 2, "{two}");
    assert_eq!(all["success"], true, "{all}");
    assert_eq!(all["results"].as_array().unwrap().len(), 3, "{all}");
}

/// Runs every scenario and returns how many ran.
async fn all_scenarios(at: &Where) -> usize {
    let r = rig(at).await;
    skill_list_returns_json_for_names_with_quotes_and_backslashes(&r).await;
    skill_search_finds_it_by_a_quoted_term(&r).await;
    kb_recall_returns_json_for_text_with_quotes_and_backslashes(&r).await;
    kb_recall_stops_at_top_k_and_at_the_end_of_the_global(&r).await;
    an_empty_store_lists_as_an_empty_array(&r).await;
    5
}

#[tokio::test]
#[ignore = "requires live IRIS (iris-dev-iris)"]
async fn the_store_readers_answer_on_iris_dev_iris() {
    assert_eq!(all_scenarios(&Where::live()).await, 5);
}

/// The same scenarios on the throwaway container: 2026.2, and the version `IAD_THROWAWAY_IMAGE`
/// names (the release gate runs it with `intersystemsdc/iris-community:2025.3`).
#[tokio::test]
#[ignore = "requires Docker (starts the throwaway IRIS)"]
async fn the_store_readers_answer_on_the_throwaway() {
    let Some(t) = ThrowawayIris::start() else {
        return;
    };
    assert_eq!(all_scenarios(&Where::throwaway(&t)).await, 5);
}
