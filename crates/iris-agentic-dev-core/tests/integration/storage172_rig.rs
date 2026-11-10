//! Live rig for the spec 172 fixtures (`tests/fixtures/storage172/`).
//!
//! A fixture is two files, written for this repository with an invented domain:
//! - `classes/<Name>.cls`: the class exactly as IRIS stores it (the text a GET returns after a
//!   PUT and compile), so loading it is also a byte-for-byte round-trip check;
//! - `seed/<Name>.json`: `{"class", "globals": [{"global", "subscripts", "value"}], "question",
//!   "answer": [ {column: value} ]}`. A seed entry may carry `"lb": [..]` instead of `"value"` for
//!   a `$LIST` node; `"build_indexes": ["<Index>"]` builds those indexes after the compile. Each
//!   `global` is killed before seeding and after the test.

use crate::live_rig_171::Rig;
use serde_json::{json, Value};
use std::path::PathBuf;

fn fixture_dir() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/storage172")
}

pub struct Fixture {
    pub name: String,
    pub class: String,
    pub cls_text: String,
    pub seed: Value,
}

impl Fixture {
    /// Read both files of fixture `name`; panics with the path when one is missing or malformed.
    pub fn read(name: &str) -> Self {
        let dir = fixture_dir();
        let cls_path = dir.join("classes").join(format!("{name}.cls"));
        let seed_path = dir.join("seed").join(format!("{name}.json"));
        let cls_text = std::fs::read_to_string(&cls_path)
            .unwrap_or_else(|e| panic!("{}: {e}", cls_path.display()));
        let seed: Value = serde_json::from_str(
            &std::fs::read_to_string(&seed_path)
                .unwrap_or_else(|e| panic!("{}: {e}", seed_path.display())),
        )
        .unwrap_or_else(|e| panic!("{}: {e}", seed_path.display()));
        let class = seed["class"]
            .as_str()
            .unwrap_or_else(|| panic!("{}: no class", seed_path.display()))
            .to_string();
        Fixture {
            name: name.to_string(),
            class,
            cls_text,
            seed,
        }
    }

    /// Every distinct global the seed sets, plus its `kill` list (globals an index build writes).
    pub fn globals(&self) -> Vec<String> {
        let mut out: Vec<String> = self.seed["globals"]
            .as_array()
            .into_iter()
            .flatten()
            .filter_map(|g| g["global"].as_str().map(str::to_string))
            .chain(
                self.seed["kill"]
                    .as_array()
                    .into_iter()
                    .flatten()
                    .filter_map(|g| g.as_str().map(str::to_string)),
            )
            .collect();
        out.sort();
        out.dedup();
        out
    }
}

/// Delete the class (if present) and kill every global the seed names. Returns what failed, so a
/// caller can assert on a clean drop.
pub async fn drop_fixture(r: &Rig, f: &Fixture) -> Vec<String> {
    let mut problems = Vec::new();
    let del = r
        .call(
            "iris_doc",
            json!({"mode": "delete", "name": format!("{}.cls", f.class), "namespace": "USER"}),
        )
        .await;
    if del["success"] != true
        && !del.to_string().contains("not found")
        && !del.to_string().contains("404")
    {
        // A class that was never loaded is not an error; anything else is.
        let gone = r
            .call(
                "iris_doc",
                json!({"mode": "get", "name": format!("{}.cls", f.class), "namespace": "USER"}),
            )
            .await;
        if gone["success"] == true {
            problems.push(format!("delete {}: {del}", f.class));
        }
    }
    for g in f.globals() {
        let k = r
            .call(
                "iris_global",
                json!({"action": "kill", "global_name": g, "namespace": "USER"}),
            )
            .await;
        if k["success"] != true {
            problems.push(format!("kill {g}: {k}"));
        }
    }
    problems
}

fn os_literal(s: &str) -> String {
    format!("\"{}\"", s.replace('"', "\"\""))
}

/// `^G("a",1)` for a seed entry's global and subscripts (numbers unquoted).
fn node_ref(g: &Value) -> String {
    let global = g["global"].as_str().unwrap_or_default();
    let subs: Vec<String> = g["subscripts"]
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(Value::as_str)
        .map(|s| {
            if s.parse::<i64>().is_ok() {
                s.to_string()
            } else {
                os_literal(s)
            }
        })
        .collect();
    if subs.is_empty() {
        global.to_string()
    } else {
        format!("{global}({})", subs.join(","))
    }
}

/// `$lb(...)` for a seed entry's `lb` list: strings quoted, numbers as written.
fn lb_expr(items: &[Value]) -> String {
    let parts: Vec<String> = items
        .iter()
        .map(|v| match v {
            Value::String(s) => os_literal(s),
            other => other.to_string(),
        })
        .collect();
    format!("$lb({})", parts.join(","))
}

/// Drop, seed, PUT and compile fixture `f`, then GET it back. Returns the stored class text.
pub async fn load_fixture(r: &Rig, f: &Fixture) -> String {
    drop_fixture(r, f).await;
    for g in f.seed["globals"].as_array().into_iter().flatten() {
        let set = if let Some(items) = g.get("lb").and_then(Value::as_array) {
            // `iris_global set` stores strings only; a $LIST node is set with ObjectScript.
            let code = format!("Set {}={} Write \"SET\"", node_ref(g), lb_expr(items));
            r.call("iris_execute", json!({"code": code, "namespace": "USER"}))
                .await
        } else {
            let mut args = json!({"action": "set", "global_name": g["global"],
                "value": g["value"], "namespace": "USER"});
            if let Some(subs) = g.get("subscripts") {
                args["subscripts"] = subs.clone();
            }
            r.call("iris_global", args).await
        };
        assert_eq!(set["success"], true, "{}: seeding {g}: {set}", f.name);
    }
    let put = r
        .call(
            "iris_doc",
            json!({"mode": "put", "name": format!("{}.cls", f.class), "content": f.cls_text,
                "compile": true, "namespace": "USER"}),
        )
        .await;
    assert_eq!(put["success"], true, "{}: put and compile: {put}", f.name);
    // A seed can name index maps to build after the compile (a bitmap cannot be seeded as text).
    // `%BuildIndices` takes the class's Index names, so each map has a matching Index definition.
    for idx in f.seed["build_indexes"].as_array().into_iter().flatten() {
        let code = format!(
            "Set sc=##class({}).%BuildIndices($lb({})) Write $Select(sc=1:\"BUILT\",1:$System.Status.GetErrorText(sc))",
            f.class,
            os_literal(idx.as_str().unwrap_or_default())
        );
        let built = r
            .call("iris_execute", json!({"code": code, "namespace": "USER"}))
            .await;
        assert!(
            built["output"]
                .as_str()
                .unwrap_or_default()
                .contains("BUILT"),
            "{}: building {idx}: {built}",
            f.name
        );
    }
    let got = r
        .call(
            "iris_doc",
            json!({"mode": "get", "name": format!("{}.cls", f.class), "namespace": "USER"}),
        )
        .await;
    assert_eq!(got["success"], true, "{}: get: {got}", f.name);
    got["content"]
        .as_str()
        .map(str::to_string)
        .or_else(|| {
            got["content"].as_array().map(|lines| {
                lines
                    .iter()
                    .filter_map(Value::as_str)
                    .collect::<Vec<_>>()
                    .join("\n")
            })
        })
        .unwrap_or_else(|| panic!("{}: get returned no content: {got}", f.name))
}

/// Run the seed's question and return its rows (objects keyed by column name).
pub async fn ask(r: &Rig, f: &Fixture) -> Vec<Value> {
    let q = r
        .call(
            "iris_query",
            json!({"query": f.seed["question"], "namespace": "USER"}),
        )
        .await;
    assert_eq!(q["success"], true, "{}: question: {q}", f.name);
    q["rows"].as_array().cloned().unwrap_or_default()
}

/// The whole check for one fixture: load it, compare the stored text with the committed file,
/// ask the known question, drop it, then assert. Cleans up before asserting.
pub async fn check_fixture(r: &Rig, name: &str) {
    let f = Fixture::read(name);
    let stored = load_fixture(r, &f).await;
    let rows = ask(r, &f).await;
    let problems = drop_fixture(r, &f).await;
    assert!(problems.is_empty(), "{name}: cleanup: {problems:?}");
    assert_eq!(
        stored.trim_end(),
        f.cls_text.trim_end(),
        "{name}: IRIS stored a different text than the committed fixture (re-author it)"
    );
    let expected = f.seed["answer"].as_array().cloned().unwrap_or_default();
    assert!(!expected.is_empty(), "{name}: the seed has no known answer");
    assert_eq!(
        rows, expected,
        "{name}: known question {}",
        f.seed["question"]
    );
}
