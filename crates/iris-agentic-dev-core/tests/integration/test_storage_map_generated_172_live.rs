//! T105: the compile oracle. Each row of the pairwise covering array over `dimensions.toml`
//! becomes a class (`storage172_gen::class::class_for`); IRIS compiles it, through Atelier
//! directly so IRIS alone decides, and its known question is asked. A class that compiles and
//! answers is a generated fixture (`generated/classes`, `generated/seed`); a class that does not
//! names the pairs no passing class has as suspects, which go to `forbidden.toml` with the IRIS
//! error numbers, and the array is rebuilt without them until no new pair appears.
//!
//! With `IAD_REGEN_GENERATED=1` the files are written. Without it, the test regenerates and
//! asserts the committed files equal what it finds. Every class is dropped afterwards.
//!
//! Run: `IRIS_HOST=localhost IRIS_WEB_PORT=52780 IAD_REGEN_GENERATED=1 cargo test --features \
//!   testing --test integration test_storage_map_generated_172 -- --test-threads=1 --include-ignored`

use crate::live_rig_171::{rig, Rig, Where};
use crate::storage172_gen::class::{class_for, seed_json, GenClass, NodeValue};
use crate::storage172_gen::pairwise::pairwise;
use crate::storage172_gen::{dims, fixture_dir, forbidden, Dim};
use iris_agentic_dev_core::tools::Toolset;
use serde_json::{json, Value};
use std::collections::{BTreeMap, BTreeSet};

const SEED: u64 = 172;

fn os_lit(s: &str) -> String {
    let mut out = String::from("\"");
    for ch in s.chars() {
        match ch {
            '"' => out.push_str("\"\""),
            '\u{1}' => out.push_str("\"_$C(1)_\""),
            c => out.push(c),
        }
    }
    out.push('"');
    out
}

/// Kill the globals, seed the nodes, PUT and compile the class through Atelier. `Ok` with the
/// stored text, or the IRIS errors.
async fn put_and_compile(
    r: &Rig,
    conn: &iris_agentic_dev_core::iris::connection::IrisConnection,
    client: &reqwest::Client,
    g: &GenClass,
) -> Result<String, Vec<String>> {
    let kill = format!("Kill {},{}I,{}S Write 1", g.global, g.global, g.global);
    r.call("iris_execute", json!({"code": kill, "namespace": "USER"}))
        .await;
    for (subs, v) in &g.nodes {
        let node = format!(
            "{}({})",
            g.global,
            subs.iter()
                .map(|s| if s.parse::<i64>().is_ok() {
                    s.clone()
                } else {
                    os_lit(s)
                })
                .collect::<Vec<_>>()
                .join(",")
        );
        let value = match v {
            NodeValue::Text(t) => os_lit(t),
            NodeValue::List(items) => format!(
                "$lb({})",
                items
                    .iter()
                    .map(|i| os_lit(i))
                    .collect::<Vec<_>>()
                    .join(",")
            ),
        };
        let set = r
            .call(
                "iris_execute",
                json!({"code": format!("Set {node}={value} Write 1"), "namespace": "USER"}),
            )
            .await;
        assert_eq!(set["success"], true, "{}: seed {node}: {set}", g.name);
    }
    let doc = format!("{}.cls", g.name);
    let url = conn.versioned_ns_url("USER", &format!("/doc/{doc}?ignoreConflict=1"));
    let lines: Vec<&str> = g.cls_text.lines().collect();
    let put = client
        .put(&url)
        .basic_auth(&conn.username, Some(&conn.password))
        .json(&json!({"enc": false, "content": lines}))
        .send()
        .await
        .expect("PUT");
    if !put.status().is_success() {
        return Err(vec![format!("PUT HTTP {}", put.status())]);
    }
    let compiled = conn
        .compile_document(&doc, "USER", "cuk", client)
        .await
        .expect("compile");
    if !compiled.success() {
        return Err(compiled.errors);
    }
    for idx in &g.build_indexes {
        let code = format!(
            "Set sc=##class({}).%BuildIndices($lb({})) Write $Select(sc=1:\"BUILT\",1:$System.Status.GetErrorText(sc))",
            g.name,
            os_lit(idx)
        );
        let b = r
            .call("iris_execute", json!({"code": code, "namespace": "USER"}))
            .await;
        if !b["output"].to_string().contains("BUILT") {
            return Err(vec![format!("%BuildIndices: {b}")]);
        }
    }
    let got = r
        .call(
            "iris_doc",
            json!({"mode": "get", "name": doc, "namespace": "USER"}),
        )
        .await;
    let text = match &got["content"] {
        Value::String(s) => s.clone(),
        Value::Array(a) => a
            .iter()
            .filter_map(Value::as_str)
            .collect::<Vec<_>>()
            .join("\n"),
        _ => String::new(),
    };
    Ok(text)
}

async fn drop_class(r: &Rig, g: &GenClass) {
    r.call(
        "iris_doc",
        json!({"mode": "delete", "name": format!("{}.cls", g.name), "namespace": "USER"}),
    )
    .await;
    let kill = format!("Kill {},{}I,{}S Write 1", g.global, g.global, g.global);
    r.call("iris_execute", json!({"code": kill, "namespace": "USER"}))
        .await;
}

/// The (dimension, value) pairs of a row.
fn row_pairs(dims: &[Dim], row: &[usize]) -> BTreeSet<(String, String, String, String)> {
    let mut out = BTreeSet::new();
    for i in 0..dims.len() {
        for j in i + 1..dims.len() {
            out.insert((
                dims[i].name.clone(),
                dims[i].values[row[i]].clone(),
                dims[j].name.clone(),
                dims[j].values[row[j]].clone(),
            ));
        }
    }
    out
}

fn error_numbers(errors: &[String]) -> Vec<u32> {
    let re = regex::Regex::new(r"#(\d+)").unwrap();
    let mut n: Vec<u32> = errors
        .iter()
        .flat_map(|e| {
            re.captures_iter(e)
                .filter_map(|c| c[1].parse().ok())
                .collect::<Vec<_>>()
        })
        .filter(|n| *n != 5030)
        .collect();
    n.sort();
    n.dedup();
    n
}

#[tokio::test]
#[ignore = "live IRIS"]
async fn test_storage_map_generated_172_compile_oracle() {
    use iris_agentic_dev_core::iris::connection::{DiscoverySource, IrisConnection};
    let regen = std::env::var("IAD_REGEN_GENERATED").is_ok_and(|v| v == "1");
    let r = rig(&Where::live(), Toolset::Merged).await;
    let t = iris_agentic_dev_core::testing::LiveTarget::from_env();
    let mut conn = IrisConnection::new(
        t.base_url(),
        &t.namespace,
        &t.username,
        &t.password,
        DiscoverySource::ExplicitFlag,
    );
    conn.probe().await;
    let client = conn
        .client(iris_agentic_dev_core::iris::http::ClientKind::Call)
        .expect("client");
    let d = dims();
    let mut banned: BTreeMap<(String, String, String, String), Vec<u32>> = BTreeMap::new();
    let mut passed: Vec<(GenClass, String)> = Vec::new();
    let mut wrong_answers = Vec::new();
    // Rebuild until a round finds no new forbidden pair.
    for _round in 0..4 {
        let mut fb = forbidden();
        fb.extend(
            banned
                .keys()
                .map(|(a, av, b, bv)| crate::storage172_gen::Pair {
                    a: (a.clone(), av.clone()),
                    b: (b.clone(), bv.clone()),
                }),
        );
        let rows = pairwise(&d, &fb, SEED);
        passed.clear();
        let mut failed: Vec<(Vec<usize>, Vec<u32>)> = Vec::new();
        let mut ok_pairs = BTreeSet::new();
        for (n, row) in rows.iter().enumerate() {
            let g = class_for(row, &d, n + 1);
            match put_and_compile(&r, &conn, &client, &g).await {
                Ok(text) => {
                    let q = r
                        .call(
                            "iris_query",
                            json!({"query": g.question, "namespace": "USER"}),
                        )
                        .await;
                    if q["rows"] == json!(g.answer) {
                        ok_pairs.extend(row_pairs(&d, row));
                        passed.push((g.clone(), text));
                    } else {
                        wrong_answers
                            .push(format!("{}: {} gave {}", g.name, g.question, q["rows"]));
                    }
                }
                Err(errors) => failed.push((row.clone(), error_numbers(&errors))),
            }
            drop_class(&r, &g).await;
        }
        let before = banned.len();
        for (row, numbers) in &failed {
            for p in row_pairs(&d, row).difference(&ok_pairs) {
                banned.entry(p.clone()).or_insert_with(|| numbers.clone());
            }
        }
        if banned.len() == before {
            break;
        }
    }
    assert!(
        wrong_answers.is_empty(),
        "classes that compiled but answered wrongly:\n{}",
        wrong_answers.join("\n")
    );
    assert!(
        passed.len() >= 10,
        "only {} generated classes compiled",
        passed.len()
    );

    let gen = fixture_dir().join("generated");
    let forbidden_text = {
        let mut s = String::from(
            "# Pairs IRIS refuses to compile together, found by the compile oracle (T105).\n",
        );
        for ((a, av, b, bv), numbers) in &banned {
            s.push_str(&format!(
                "\n[[pair]]\ndim_a = \"{a}\"\nvalue_a = \"{av}\"\ndim_b = \"{b}\"\nvalue_b = \"{bv}\"\nerrors = {numbers:?}\n"
            ));
        }
        s
    };
    if regen {
        let _ = std::fs::remove_dir_all(&gen);
        std::fs::create_dir_all(gen.join("classes")).unwrap();
        std::fs::create_dir_all(gen.join("seed")).unwrap();
        for (g, text) in &passed {
            let short = g.name.rsplit('.').next().unwrap();
            std::fs::write(
                gen.join("classes").join(format!("{short}.cls")),
                format!("{}\n", text.trim_end()),
            )
            .unwrap();
            std::fs::write(
                gen.join("seed").join(format!("{short}.json")),
                serde_json::to_string_pretty(&seed_json(g)).unwrap() + "\n",
            )
            .unwrap();
        }
        // Only new pairs: the committed file already held the ones fed in.
        if !banned.is_empty() {
            let path = fixture_dir().join("forbidden.toml");
            let mut text = std::fs::read_to_string(&path).unwrap_or_default();
            if text.is_empty() {
                text = forbidden_text;
            } else {
                text.push_str(
                    &forbidden_text
                        .lines()
                        .skip(1)
                        .collect::<Vec<_>>()
                        .join("\n"),
                );
            }
            std::fs::write(path, text).unwrap();
        }
        return;
    }
    assert!(
        banned.is_empty(),
        "new forbidden pairs (regenerate with IAD_REGEN_GENERATED=1):\n{forbidden_text}"
    );
    let mut mismatch = Vec::new();
    for (g, text) in &passed {
        let short = g.name.rsplit('.').next().unwrap();
        let cls = std::fs::read_to_string(gen.join("classes").join(format!("{short}.cls")))
            .unwrap_or_default();
        if cls.trim_end() != text.trim_end() {
            mismatch.push(format!("{short}.cls"));
        }
        let seed = std::fs::read_to_string(gen.join("seed").join(format!("{short}.json")))
            .unwrap_or_default();
        if seed != serde_json::to_string_pretty(&seed_json(g)).unwrap() + "\n" {
            mismatch.push(format!("{short}.json"));
        }
    }
    assert!(
        mismatch.is_empty(),
        "the committed generated files differ: {mismatch:?}"
    );
}
