//! Spec 173 T053/T054 (US6): `propose` returns `class_draft`, a class with a typed property for each
//! master field, counted on the server, with the `COLLATION` the linking found, compiled in a
//! scratch copy with the drafted maps. Each fixture's known answer comes from
//! `tests/fixtures/storage173/seed/<F>.json` (counts the fixture generator computed from its own
//! rows), never from the answer under test; the end-to-end checks ask IRIS's own SQL about the class
//! that the draft's source compiles to.
//!
//! Run: `IRIS_HOST=localhost IRIS_WEB_PORT=52780 nice -n 15 cargo test --features testing -j 2 \
//!   --test integration test_storage_map_typing_173 -- --test-threads=1 --include-ignored`

use crate::live_rig_171::Rig;
use crate::storage173_rig::*;
use crate::test_storage_map_link_173_live::live;
use iris_agentic_dev_core::tools::storage_map::typing::collation_parameter;
use serde_json::{json, Value};

async fn done(r: &Rig, f: &Fixture) {
    drop_fixture_173(r, f).await;
    drop_all(r).await;
}

/// `propose` over the fixture's master and every one of its index globals, the properties named
/// after the seed's columns.
async fn proposed(r: &Rig, f: &Fixture) -> Value {
    load_fixture_173(r, f).await;
    let master = f.seed["master_global"].as_str().unwrap();
    let out = ask_propose(
        r,
        json!({"global": master, "index_globals": f.seed["index_globals"],
               "field_names": f.columns()}),
    )
    .await;
    assert_eq!(out["success"], true, "{}: {out}", f.name);
    out
}

fn prop<'a>(out: &'a Value, name: &str) -> &'a Value {
    out["class_draft"]["properties"]
        .as_array()
        .unwrap_or_else(|| panic!("no class_draft.properties in {out}"))
        .iter()
        .find(|p| p["name"] == json!(name))
        .unwrap_or_else(|| panic!("no property {name} in {}", out["class_draft"]))
}

fn parameters(p: &Value) -> Vec<String> {
    p["parameters"]
        .as_array()
        .unwrap_or_else(|| panic!("no parameters in {p}"))
        .iter()
        .filter_map(|x| x.as_str().map(String::from))
        .collect()
}

fn number(v: &Value) -> f64 {
    v.as_f64()
        .or_else(|| v.as_str().and_then(|s| s.trim().parse().ok()))
        .unwrap_or_else(|| panic!("not a number: {v}"))
}

/// F37: every column has the type, and the counts, the seed's own rows give.
#[tokio::test]
#[ignore]
async fn f37_each_column_gets_the_type_its_values_support() {
    let r = live().await;
    let f = Fixture::read("F37");
    let out = proposed(&r, &f).await;
    let answer = &f.seed["answer"];
    let mut wrong = Vec::new();
    for c in f.columns() {
        let want = &answer["typing"][&c];
        let p = prop(&out, &c);
        let want_type = answer["types"][&c].as_str().unwrap();
        if p["type"] != json!(format!("%{}", want_type.trim_start_matches('%'))) {
            wrong.push(format!("{c}: type {} want {want_type}", p["type"]));
            continue;
        }
        let ev = &p["evidence"];
        let params = parameters(p);
        match want_type {
            "%Integer" => {
                if *ev != json!({"integer": want["integer"], "of": want["of"]}) {
                    wrong.push(format!("{c}: evidence {ev} want {want}"));
                }
                if want["integer"] != want["of"] {
                    wrong.push(format!("{c}: the seed says {want}, not all whole"));
                }
            }
            "%Numeric" => {
                if *ev
                    != json!({"numeric": want["numeric"], "max_scale": want["max_scale"],
                              "of": want["of"]})
                {
                    wrong.push(format!("{c}: evidence {ev} want {want}"));
                }
                let scale = format!("SCALE = {}", want["max_scale"]);
                if !params.contains(&scale) {
                    wrong.push(format!("{c}: parameters {params:?} lack {scale}"));
                }
            }
            "%Date" => {
                if *ev != json!({"date_fit": want["date_fit"], "of": want["of"]}) {
                    wrong.push(format!("{c}: evidence {ev} want {want}"));
                }
                let (fit, of) = (
                    want["date_fit"].as_u64().unwrap(),
                    want["of"].as_u64().unwrap(),
                );
                if fit * 100 < of * 95 {
                    wrong.push(format!(
                        "{c}: the seed itself fits {fit} of {of}, under 95%"
                    ));
                }
            }
            _ => {
                if *ev != json!({"max_length": want["max_length"]}) {
                    wrong.push(format!("{c}: evidence {ev} want {want}"));
                }
                let longest = want["max_length"].as_u64().unwrap();
                let maxlen = params
                    .iter()
                    .find_map(|s| s.strip_prefix("MAXLEN = "))
                    .and_then(|n| n.parse::<u64>().ok());
                match maxlen {
                    Some(n) if n >= longest => {}
                    other => {
                        wrong.push(format!("{c}: MAXLEN {other:?} under the longest {longest}"))
                    }
                }
            }
        }
    }
    done(&r, &f).await;
    assert!(wrong.is_empty(), "{}", wrong.join("\n"));
}

/// The text columns of F37 are indexed through sqlupper, sqlstring and exact; the class draft carries
/// what the linking found, and nothing for the default.
#[tokio::test]
#[ignore]
async fn f37_the_collation_follows_how_the_index_files_the_field() {
    let r = live().await;
    let f = Fixture::read("F37");
    let out = proposed(&r, &f).await;
    // The premise: all three links are inferred, or there is no evidence to follow.
    for item in out["indexes"].as_array().unwrap() {
        assert_eq!(item["verdict"], "inferred", "{item}");
    }
    let want = &f.seed["answer"]["collations"];
    for c in ["Name", "Alias", "City"] {
        let collations: Vec<String> = parameters(prop(&out, c))
            .into_iter()
            .filter(|p| p.starts_with("COLLATION"))
            .collect();
        match want[c].as_str() {
            Some(text) => assert_eq!(collations, vec![text.to_string()], "{c}"),
            None => assert!(collations.is_empty(), "{c}: {collations:?}"),
        }
    }
    // A field no index links carries no collation.
    for c in ["Qty", "Price", "Placed", "Stamp", "Note"] {
        assert!(
            !parameters(prop(&out, c))
                .iter()
                .any(|p| p.starts_with("COLLATION")),
            "{c}"
        );
    }
    done(&r, &f).await;
}

/// The draft compiled in a scratch copy, and its source compiles for real and answers SQL the way
/// the seed's rows say: the types and the collation do what the draft claims.
#[tokio::test]
#[ignore]
async fn f37_the_draft_compiled_and_its_source_reads_the_master() {
    let r = live().await;
    let f = Fixture::read("F37");
    let out = proposed(&r, &f).await;
    let draft = &out["class_draft"];
    assert_eq!(draft["compiled"], true, "{draft}");
    let source = draft["source"].as_str().unwrap();
    assert!(source.contains("Storage SQLStorage"), "{source}");
    // The maps the draft carries: the master and one per index.
    for name in ["Master", "X", "XA", "XC"] {
        assert!(
            source.contains(&format!("<SQLMap name=\"{name}\">")),
            "no map {name} in the source:\n{source}"
        );
    }
    let real = source.replace("Class Draft.IadSm173F37 ", "Class IadSm173.F37Draft ");
    assert_ne!(
        real, source,
        "the class name is not the one expected:\n{source}"
    );
    put_class(&r, "IadSm173.F37Draft", &real).await;

    let answer = &f.seed["answer"];
    let sums = select(
        &r,
        "SELECT COUNT(*) AS n, SUM(Qty) AS q, SUM(Price) AS p, MAX(Placed) AS m FROM IadSm173.F37Draft",
    )
    .await;
    let row = &sums[0];
    assert_eq!(number(&row["n"]), f.rows().len() as f64, "{row}");
    assert_eq!(number(&row["q"]), number(&answer["sums"]["Qty"]), "{row}");
    assert!(
        (number(&row["p"]) - number(&answer["sums"]["Price"])).abs() < 0.005,
        "{row} want {}",
        answer["sums"]["Price"]
    );
    // %Date: the day number reads as the date IRIS's own function gives for it.
    let day = answer["placed_max"].as_i64().unwrap();
    let date = exec(&r, &format!("Write $ZDATE({day},3)")).await;
    assert_eq!(row["m"].as_str().unwrap().trim(), date.trim(), "{row}");

    // COLLATION = "EXACT": `boston` is not `Boston`.
    let exact = f.rows().iter().filter(|x| x[2] == json!("Boston")).count();
    let sql = select(
        &r,
        "SELECT COUNT(*) AS n FROM IadSm173.F37Draft WHERE City = 'Boston'",
    )
    .await;
    assert!(exact > 0 && exact < f.rows().len());
    assert_eq!(number(&sql[0]["n"]), exact as f64, "{}", sql[0]);
    done(&r, &f).await;
}

/// Every property of the draft is marked a guess, and no sampled value is in the draft.
#[tokio::test]
#[ignore]
async fn f37_every_property_is_a_guess_and_no_value_is_in_the_draft() {
    let r = live().await;
    let f = Fixture::read("F37");
    let out = proposed(&r, &f).await;
    let draft = &out["class_draft"];
    let listed = draft["guesses"].to_string();
    for c in f.columns() {
        let path = format!("/class_draft/properties/{c}");
        assert!(
            listed.contains(&path),
            "{path} is not in class_draft.guesses: {listed}"
        );
    }
    let text = draft.to_string();
    for row in f.rows() {
        for v in row {
            if let Some(s) = v.as_str().filter(|s| s.trim().len() >= 4) {
                assert!(
                    !text.contains(s.trim()),
                    "the value {s:?} is in the class draft"
                );
            }
        }
    }
    done(&r, &f).await;
}

/// F26: two identical columns, a small integer column and a day-number column. Nothing that is not a
/// day number becomes a date.
#[tokio::test]
#[ignore]
async fn f26_data_that_only_looks_like_dates_makes_no_date() {
    let r = live().await;
    let f = Fixture::read("F26");
    load_fixture_173(&r, &f).await;
    let master = f.seed["master_global"].as_str().unwrap();
    let out = ask_propose(&r, json!({"global": master, "field_names": f.columns()})).await;
    assert_eq!(out["success"], true, "{out}");
    assert_eq!(
        out["class_draft"]["compiled"], true,
        "{}",
        out["class_draft"]
    );
    // The window IRIS's own function gives, and the seed's columns against it.
    let lo: i64 = exec(&r, "Write $ZDATEH(\"1900-01-01\",3)")
        .await
        .trim()
        .parse()
        .unwrap();
    let hi: i64 = exec(&r, "Write $ZDATEH(\"2100-12-31\",3)")
        .await
        .trim()
        .parse()
        .unwrap();
    let column = |name: &str| -> Vec<Value> {
        let at = f.columns().iter().position(|c| c == name).unwrap();
        f.rows().iter().map(|row| row[at].clone()).collect()
    };
    let age: Vec<i64> = column("Age").iter().map(|v| v.as_i64().unwrap()).collect();
    let born: Vec<i64> = column("Born").iter().map(|v| v.as_i64().unwrap()).collect();
    assert!(
        age.iter().all(|d| *d < lo),
        "premise: Age is below the window"
    );
    assert!(
        born.iter().all(|d| (lo..=hi).contains(d)),
        "premise: Born is inside it"
    );
    assert_eq!(
        column("City"),
        column("Status"),
        "premise: two identical columns"
    );

    for name in ["Name", "City", "Status"] {
        assert_eq!(prop(&out, name)["type"], "%String", "{name}");
    }
    let age_prop = prop(&out, "Age");
    assert_eq!(age_prop["type"], "%Integer", "{age_prop}");
    assert_eq!(age_prop["evidence"], json!({"integer": 40, "of": 40}));
    let born_prop = prop(&out, "Born");
    assert_eq!(born_prop["type"], "%Date", "{born_prop}");
    assert_eq!(born_prop["evidence"], json!({"date_fit": 40, "of": 40}));
    assert_eq!(
        prop(&out, "City")["evidence"],
        prop(&out, "Status")["evidence"]
    );
    done(&r, &f).await;
}

/// The collation names `collation_parameter` writes are the ones IRIS accepts (checked by a real
/// compile), and a name it does not know is refused by the same compile.
#[tokio::test]
#[ignore]
async fn the_collation_names_compile_and_an_unknown_one_does_not() {
    let r = live().await;
    let cases: [(&str, Option<u32>); 10] = [
        ("exact", None),
        ("sqlupper", Some(4)),
        ("sqlstring", None),
        ("sqlstring", Some(3)),
        ("string", None),
        ("string", Some(3)),
        ("alphaup", None),
        ("upper", None),
        ("mvr", None),
        ("truncate", Some(5)),
    ];
    assert_eq!(collation_parameter("sqlupper", None), Ok(None));
    let mut body = String::new();
    for (i, (t, len)) in cases.iter().enumerate() {
        let param = collation_parameter(t, *len)
            .unwrap()
            .unwrap_or_else(|| panic!("{t}: no parameter"));
        body.push_str(&format!("Property P{i} As %String({param});\n\n"));
    }
    let class = format!("Class IadSm173.CollProbe Extends %Persistent\n{{\n\n{body}}}\n");
    let (ok, answer) = try_class(&r, "IadSm173.CollProbe", &class).await;
    assert!(
        ok,
        "IRIS refused a collation name the draft writes: {answer}"
    );

    let bad = "Class IadSm173.CollBad Extends %Persistent\n{\n\nProperty P As %String(COLLATION = \"BOGUSXX\");\n\n}\n";
    let (bad_ok, bad_answer) = try_class(&r, "IadSm173.CollBad", bad).await;
    assert!(!bad_ok, "an unknown collation compiled: {bad_answer}");
    assert!(
        bad_answer.contains("Collation") && bad_answer.contains("BOGUSXX"),
        "{bad_answer}"
    );
    drop_all(&r).await;
}
