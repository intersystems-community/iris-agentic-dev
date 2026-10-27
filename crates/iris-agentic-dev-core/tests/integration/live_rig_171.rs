//! A live rig shared by the R15 and E7 tests: an `IrisTools` pointed at `iris-dev-iris` (or at the
//! throwaway container) with the write and destructive tiers declared, so a test can create and
//! drop its own scratch documents.

use iris_agentic_dev_core::iris::config_sources::{ConfigSnapshot, LoadInputs, CONFIG_FILE_NAME};
use iris_agentic_dev_core::iris::connection::{DiscoverySource, IrisConnection};
use iris_agentic_dev_core::skills::SkillRegistry;
use iris_agentic_dev_core::testing::{LiveTarget, ThrowawayIris};
use iris_agentic_dev_core::tools::{IrisTools, Toolset};
use serde_json::{json, Value};

/// Where the scenarios run: `iris-dev-iris`, or the throwaway container (the default image and
/// `IAD_THROWAWAY_IMAGE`, so the same scenarios run on 2025.3).
pub struct Where {
    base_url: String,
    user: String,
    password: String,
    namespace: String,
}

impl Where {
    pub fn live() -> Self {
        let t = LiveTarget::from_env();
        t.assert_running();
        Where {
            base_url: t.base_url(),
            user: t.username.clone(),
            password: t.password.clone(),
            namespace: t.namespace.clone(),
        }
    }

    pub fn throwaway(t: &ThrowawayIris) -> Self {
        Where {
            base_url: format!("http://{}:{}", t.host, t.web_port),
            user: t.username.to_string(),
            password: t.password.to_string(),
            namespace: "USER".to_string(),
        }
    }
}

pub struct Rig {
    pub tools: IrisTools,
    _dir: tempfile::TempDir,
}

/// Where the scenarios run: `iris-dev-iris`, or the throwaway container (the default image and
/// `IAD_THROWAWAY_IMAGE`, so the same scenarios run on 2025.3).
pub async fn rig(at: &Where, toolset: Toolset) -> Rig {
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
        Some(conn),
        SkillRegistry::new(),
        toolset,
        None,
        None,
        false,
        declared,
    )
    .unwrap();
    tools.set_config(inputs, snapshot);
    Rig { tools, _dir: dir }
}

impl Rig {
    pub async fn call(&self, tool: &str, args: Value) -> Value {
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
