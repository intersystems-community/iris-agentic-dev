//! T096 (US10): the live answers of `compare`, `preview` and `propose` are pinned on `Member` and
//! `BookMulti`, values masked, in `tests/fixtures/storage172/golden/<Name>.<action>.live.json`.
//! The scratch class name and the identity block change on every run and are left out.
//! `IAD_REGEN_GOLDEN=1` rewrites the files.
//!
//! Run: `IRIS_HOST=localhost IRIS_WEB_PORT=52780 cargo test --features testing --test integration \
//!   test_storage_map_golden_172 -- --test-threads=1 --include-ignored`

use crate::live_rig_171::{rig, Where};
use crate::storage172_rig::{drop_fixture, load_fixture, Fixture};
use iris_agentic_dev_core::tools::Toolset;
use serde_json::{json, Value};

fn stable(mut v: Value) -> Value {
    if let Some(o) = v.as_object_mut() {
        o.remove("scratch_class");
        o.remove("identity");
    }
    v
}

#[tokio::test]
#[ignore = "live IRIS"]
async fn test_storage_map_golden_172_live_answers() {
    let r = rig(&Where::live(), Toolset::Merged).await;
    let golden = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures/storage172/golden");
    let regen = std::env::var("IAD_REGEN_GOLDEN").is_ok_and(|v| v == "1");
    let mut failures = Vec::new();
    for name in ["Member", "BookMulti"] {
        let f = Fixture::read(name);
        load_fixture(&r, &f).await;
        let read = r
            .call(
                "iris_storage_map",
                json!({"action": "read", "class": f.class, "namespace": "USER"}),
            )
            .await;
        let global = read["storage"]["maps"][0]["global"].clone();
        let calls = [
            (
                "compare",
                json!({"action": "compare", "class": f.class, "namespace": "USER"}),
            ),
            (
                "preview",
                json!({"action": "preview", "class": f.class, "namespace": "USER"}),
            ),
            (
                "propose",
                json!({"action": "propose", "global": global, "namespace": "USER"}),
            ),
        ];
        for (action, args) in calls {
            let got = stable(r.call("iris_storage_map", args).await);
            let path = golden.join(format!("{name}.{action}.live.json"));
            let fresh = serde_json::to_string_pretty(&got).unwrap() + "\n";
            if got["success"] != true {
                failures.push(format!("{name} {action}: {got}"));
            } else if got.to_string().contains("Alice") || got.to_string().contains("Portland") {
                failures.push(format!("{name} {action}: a value reached a masked answer"));
            } else if regen {
                std::fs::write(&path, &fresh).unwrap();
            } else {
                match std::fs::read_to_string(&path) {
                    Ok(stored) if stored == fresh => {}
                    Ok(_) => failures.push(format!("{}: differs", path.display())),
                    Err(_) => failures.push(format!("{}: missing", path.display())),
                }
            }
        }
        drop_fixture(&r, &f).await;
    }
    assert!(failures.is_empty(), "{}", failures.join("\n"));
}
