//! Spec 173 T058 (US9): the public corpus. Every fixture under `tests/fixtures/storage173/seed/`
//! runs through `propose` the way its own live test drives it, and its answer is held to the known
//! answer the seed file states (SC-001), never to the answer under test. The table below names how
//! each fixture is driven; what each one must return comes from its seed (`answer`, `rows`,
//! `killed_rows`, `post_load`) and from the class text the fixture declares.
//!
//! - SC-001: at least 30 fixtures, every one of them gives its known answer (the count is asserted
//!   before the first fixture loads, and a seed with no driver, or a driver with no seed, fails).
//! - SC-002: every `inferred` draft compiled in the scratch copy and found its row, and where the
//!   fixture's class declares the same index map the draft is that map, expression by expression.
//! - SC-003: no fixture of the false-link set (ties, constants, unrelated or half-orphaned indexes)
//!   is called `inferred` anywhere in its answer, and no `inferred` subscript names a field the seed
//!   does not.
//! - Each answer is compared with `tests/fixtures/storage173/golden/<fixture>.json`;
//!   `IAD_REGEN_GOLDEN=1` writes the files (a golden that does not exist is created only then).
//!
//! Run: `IRIS_HOST=localhost IRIS_WEB_PORT=52780 nice -n 15 cargo test --features testing -j 2 \
//!   --test integration test_storage_map_corpus_173 -- --test-threads=1 --include-ignored`

use crate::live_rig_171::{rig_with, Rig, Where};
use crate::storage173_rig::*;
use crate::test_storage_map_link_173_live::{draft_name, live, opt};
use iris_agentic_dev_core::tools::Toolset;
use serde_json::{json, Value};
use std::path::PathBuf;
use std::time::Instant;

/// The operator file that lets a call ask for sampled values (still refused without
/// `include_values` and `acknowledgePhi`).
const ALLOW: &str = "write_tools_enabled = true\ndestructive_tools_enabled = true\n\n[policy.default]\ndataPolicy = \"allow\"\n";

fn fixture_dir() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/storage173")
}

/// How a fixture is driven. The choice is the one its own live test makes.
#[derive(Clone, Copy, Debug, PartialEq)]
enum Drive {
    /// `propose global=<master> index_globals=<the seed's>`: link, verdicts, drafts.
    Link,
    /// F22 to F25: an index with fewer nodes than the master has rows.
    Sparse,
    /// `propose global=<master>` alone: the index globals are found by name.
    Candidates,
    /// F32: the index lives inside the master.
    SubNode,
    /// F48: ten candidates, eight examined.
    Capped,
    /// F35: `propose class=...` drafts the missing index.
    ClassNew,
    /// F36: `propose class=...` finds the index already mapped.
    ClassMapped,
    /// F19, F19c, F19n, F20: bitmap indexes.
    Bitmap,
    /// F21: a master that holds bit strings.
    BitMaster,
    /// F37: typing and collation of the class draft.
    Typing,
}

/// Every fixture and how it is driven. A seed that is not in this table fails the sweep.
fn drive_of(name: &str) -> Option<Drive> {
    use Drive::*;
    Some(match name {
        "F19" | "F19c" | "F19n" | "F20" => Bitmap,
        "F21" => BitMaster,
        "F22" | "F23" | "F24" | "F25" => Sparse,
        "F32" => SubNode,
        "F35" => ClassNew,
        "F36" => ClassMapped,
        "F37" => Typing,
        "F47" => Candidates,
        "F48" => Capped,
        "F01" | "F02" | "F03" | "F04" | "F05" | "F06" | "F07" | "F08" | "F09" | "F10" | "F11"
        | "F12" | "F13" | "F14" | "F15" | "F16" | "F17" | "F18" | "F26" | "F27" | "F28" | "F29"
        | "F30" | "F31" | "F33" | "F34" | "F38" | "F39" | "F40" | "F41" | "F42" | "F43" | "F44"
        | "F45" | "F46" | "F49" | "F50" => Link,
        _ => return None,
    })
}

/// The false-link set: whatever `propose` says about these, no verdict in the answer may be
/// `inferred` (SC-003). Each one is an index that must not be taken for a link to a field: a tie
/// (F26, F29), a column that never varies (F28), too few rows (F27), an unrelated index (F30, F31),
/// a subscript made by code (F33), a half-deleted master (F46), bit strings of other rows (F19n),
/// an index 0.925 of the way to City (F50, under the 0.95 line).
const FALSE_LINKS: [&str; 10] = [
    "F19n", "F26", "F27", "F28", "F29", "F30", "F31", "F33", "F46", "F50",
];

fn seed_names() -> Vec<String> {
    let mut out: Vec<String> = std::fs::read_dir(fixture_dir().join("seed"))
        .expect("the seed directory")
        .flatten()
        .filter_map(|e| {
            let p = e.path();
            (p.extension()? == "json").then(|| p.file_stem()?.to_str().map(String::from))?
        })
        .collect();
    out.sort();
    out
}

fn read_seed(name: &str) -> Value {
    let path = fixture_dir().join("seed").join(format!("{name}.json"));
    serde_json::from_str(&std::fs::read_to_string(&path).unwrap()).unwrap()
}

/// What one fixture's run returns: the answer the golden pins, and what disagreed with the seed.
struct Outcome {
    answer: Value,
    wrong: Vec<String>,
    /// Links whose draft was compiled and queried independently (SC-002), and of those the ones
    /// whose plan was read.
    independent: usize,
    planned: usize,
}

impl Outcome {
    fn new(answer: Value) -> Self {
        Outcome {
            answer,
            wrong: Vec::new(),
            independent: 0,
            planned: 0,
        }
    }

    fn is(&mut self, what: &str, got: Value, want: Value) {
        if got != want {
            self.wrong.push(format!("{what}: got {got}, want {want}"));
        }
    }

    fn that(&mut self, ok: bool, what: impl FnOnce() -> String) {
        if !ok {
            self.wrong.push(what());
        }
    }
}

async fn propose(r: &Rig, f_name: &str, args: Value) -> Result<Value, String> {
    let out = ask_propose(r, args).await;
    if out["success"] == json!(true) {
        Ok(out)
    } else {
        Err(format!("{f_name}: propose failed: {out}"))
    }
}

// ---- what the seed expects of a link ----------------------------------------------------------

/// What one index global must come back as, read from the seed.
struct Expect {
    global: String,
    id_positions: Option<Value>,
    id_form: String,
    verdict: Option<String>,
    subs: Vec<Value>,
    /// The `answer` object (or the `answer.indexes` entry) the rest is read from.
    answer: Value,
}

/// The seed's expectations, one per index global it states an answer for.
fn expectations(seed: &Value) -> Vec<Expect> {
    let a = &seed["answer"];
    if let Some(list) = a["indexes"].as_array() {
        return list
            .iter()
            .map(|i| Expect {
                global: i["global"].as_str().unwrap().to_string(),
                id_positions: i.get("id_positions").cloned(),
                id_form: "subscript".into(),
                verdict: Some("inferred".into()),
                subs: vec![
                    json!({"position": 1, "field": i["field"], "transform": i["transform"]}),
                ],
                answer: i.clone(),
            })
            .collect();
    }
    let subs: Vec<Value> = match a["subscripts"].as_array() {
        Some(s) => s.clone(),
        None if a.get("field").is_some() => vec![json!({"position": 1, "field": a["field"],
            "transform": a["transform"], "length": opt(a, "length"), "piece": opt(a, "piece")})],
        None => Vec::new(),
    };
    let tied = subs.iter().any(|s| s["verdict"] == json!("ambiguous"));
    let first = subs
        .iter()
        .filter_map(|s| s["position"].as_u64())
        .min()
        .unwrap_or(1);
    let verdict = a["link_verdict"]
        .as_str()
        .map(String::from)
        // A fixture that lists no verdict: a tie is ambiguous, a first listed position past 1 means
        // the positions before it are the constant index name, and the rest are inferred. F39 lists
        // none of its subscripts and states no verdict.
        .or_else(|| {
            if subs.is_empty() {
                None
            } else if tied {
                Some("ambiguous".into())
            } else if first > 1 {
                Some("constant".into())
            } else {
                Some("inferred".into())
            }
        });
    vec![Expect {
        global: a["index_global"].as_str().unwrap().to_string(),
        id_positions: a.get("id_positions").cloned(),
        id_form: a["id_form"].as_str().unwrap_or("subscript").to_string(),
        verdict,
        subs,
        answer: a.clone(),
    }]
}

fn sub_at(link: &Value, position: u64) -> Value {
    link["subscripts"]
        .as_array()
        .and_then(|a| a.iter().find(|s| s["position"] == json!(position)))
        .cloned()
        .unwrap_or(Value::Null)
}

fn tied_transforms(sub: &Value, pick: Option<&str>) -> Vec<String> {
    let mut got: Vec<String> = sub["alternatives"]
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(|a| a["transform"].as_str().map(String::from))
        .collect();
    got.retain(|t| Some(t.as_str()) != pick);
    got.sort();
    got
}

/// Compare one link with what the seed says, `named` turning a seed column into the name the draft
/// uses (`Field<n>` for a global, the class's own for class mode).
fn check_link(
    o: &mut Outcome,
    f: &Fixture,
    e: &Expect,
    link: &Value,
    named: &dyn Fn(&str) -> String,
) {
    let at = &e.global;
    o.is(
        &format!("{at} global"),
        opt(link, "global"),
        json!(e.global),
    );
    if let Some(v) = &e.verdict {
        o.is(&format!("{at} verdict"), opt(link, "verdict"), json!(v));
    }
    if let Some(p) = &e.id_positions {
        o.is(
            &format!("{at} id.positions"),
            opt(&link["id"], "positions"),
            p.clone(),
        );
    }
    if e.verdict.as_deref() != Some("too_few_nodes") && link["verdict"] != json!("not_linked") {
        o.is(
            &format!("{at} id.form"),
            opt(&link["id"], "form"),
            json!(e.id_form),
        );
    }
    for want in &e.subs {
        let position = want["position"].as_u64().unwrap();
        let sub = sub_at(link, position);
        let w = format!("{at} position {position}");
        let want_verdict = want.get("verdict").cloned().unwrap_or(json!("inferred"));
        o.is(
            &format!("{w} verdict"),
            opt(&sub, "verdict"),
            want_verdict.clone(),
        );
        if let Some(fields) = want["fields"].as_array() {
            // A tie between columns: each is listed (F26).
            let mut got: Vec<String> = sub["alternatives"]
                .as_array()
                .into_iter()
                .flatten()
                .filter_map(|a| a["field"].as_str().map(String::from))
                .collect();
            got.sort();
            got.dedup();
            let mut names: Vec<String> =
                fields.iter().map(|c| named(c.as_str().unwrap())).collect();
            names.sort();
            o.is(&format!("{w} tied fields"), json!(got), json!(names));
        }
        if let Some(tied) = want.get("alternatives").and_then(Value::as_array) {
            let mut want_alt: Vec<String> = tied
                .iter()
                .filter_map(|t| t.as_str().map(String::from))
                .collect();
            want_alt.sort();
            o.is(
                &format!("{w} tied transforms"),
                json!(tied_transforms(&sub, want["transform"].as_str())),
                json!(want_alt),
            );
        }
        match want["field"].as_str() {
            Some(field) => {
                o.is(
                    &format!("{w} field"),
                    opt(&sub, "field"),
                    json!(named(field)),
                );
                o.is(
                    &format!("{w} transform"),
                    opt(&sub, "transform"),
                    opt(want, "transform"),
                );
                o.is(
                    &format!("{w} length"),
                    opt(&sub, "length"),
                    opt(want, "length"),
                );
                o.is(
                    &format!("{w} piece"),
                    opt(&sub, "piece"),
                    opt(want, "piece"),
                );
                let agreement = sub["agreement"].as_f64().unwrap_or(0.0);
                match want["agreement"].as_f64() {
                    // A boundary fixture (F49) states the agreement its rewritten nodes leave.
                    Some(exact) => o.that((agreement - exact).abs() < 1e-3, || {
                        format!("{w} agreement {agreement}, want {exact}")
                    }),
                    None => o.that(agreement >= 0.95, || format!("{w} agreement {agreement}")),
                }
            }
            None if want_verdict == json!("not_explained") || want_verdict == json!("constant") => {
                o.that(
                    sub.get("field").is_none() && sub.get("transform").is_none(),
                    || format!("{w} names a field though it is {want_verdict}: {sub}"),
                );
            }
            None => {}
        }
        if want_verdict == json!("not_explained") {
            if let Some(a) = sub["agreement"].as_f64() {
                match want["agreement"].as_f64() {
                    Some(exact) => o.that((a - exact).abs() < 1e-3, || {
                        format!("{w} agreement {a}, want {exact}")
                    }),
                    None => o.that(a < 0.95, || {
                        format!("{w} agreement {a} on an unexplained subscript")
                    }),
                }
            } else if want.get("agreement").is_some() {
                o.wrong.push(format!("{w}: no agreement reported in {sub}"));
            }
        }
    }
    if let Some(covered) = e.answer["data_in_value"].as_array() {
        let want: Vec<Value> = covered
            .iter()
            .map(|c| json!({"field": named(c["field"].as_str().unwrap()), "element": c["element"]}))
            .collect();
        o.is(
            &format!("{at} data_in_value"),
            opt(link, "data_in_value"),
            json!(want),
        );
    }
    if let Some(n) = e.answer.get("null") {
        let sub = sub_at(link, n["position"].as_u64().unwrap());
        let got = &sub["null_marker"];
        o.is(
            &format!("{at} null kind"),
            opt(got, "kind"),
            n["kind"].clone(),
        );
        o.is(
            &format!("{at} null empty"),
            opt(got, "empty"),
            n["empty"].clone(),
        );
        // `sampled` is how many of the `count` empty rows the sample reaches.
        o.is(
            &format!("{at} null count"),
            opt(got, "count"),
            n["sampled"].clone(),
        );
        o.is(
            &format!("{at} null length"),
            opt(got, "length"),
            opt(n, "length"),
        );
    }
    if e.answer["no_map"] == json!(true) {
        o.that(link.get("map").is_none(), || {
            format!("{at}: a map was drafted for a link the seed says has none: {link}")
        });
    }
    if let Some(positions) = e.answer["examined_positions"].as_array() {
        let got: Vec<Value> = link["subscripts"]
            .as_array()
            .into_iter()
            .flatten()
            .map(|s| s["position"].clone())
            .collect();
        o.is(
            &format!("{at} examined positions"),
            json!(got),
            json!(positions),
        );
        o.is(
            &format!("{at} unexamined positions"),
            opt(link, "unexamined_positions"),
            e.answer["unexamined_positions"].clone(),
        );
    }
    if let Some(agreement) = e.answer["id_agreement"].as_f64() {
        let got = link["id"]["agreement"].as_f64();
        o.that(got == Some(agreement), || {
            format!("{at} id agreement {got:?}, want {agreement}")
        });
    }
    if let Some(killed) = e.answer["killed_rows"].as_array() {
        let (gone, of) = (killed.len() as u64, f.rows().len() as u64);
        if e.answer["link_verdict"] == json!("not_linked") {
            let got = link["id"]["agreement"].as_f64().unwrap_or(-1.0);
            let want = (of - gone) as f64 / of as f64;
            o.that((got - want).abs() < 1e-3, || {
                format!("{at} agreement {got}, want {want}")
            });
        } else {
            o.is(
                &format!("{at} usable nodes"),
                opt(link, "usable_nodes"),
                json!(of - gone),
            );
            if e.answer["orphan_reported"] == json!(true) {
                o.is(
                    &format!("{at} orphan count"),
                    opt(&link["orphan_nodes"], "count"),
                    json!(gone),
                );
            } else {
                o.that(link.get("orphan_nodes").is_none(), || {
                    format!("{at}: orphans reported under 5%: {link}")
                });
            }
        }
    }
    if e.verdict.as_deref() == Some("too_few_nodes") {
        o.is(
            &format!("{at} usable nodes"),
            opt(link, "usable_nodes"),
            json!(f.rows().len()),
        );
    }
}

/// SC-002: an `inferred` (or tied) link carries a drafted map that compiled in the scratch copy and
/// found its row; where the fixture's class declares a map over the same global, the draft is that
/// map, level by level.
fn check_draft(
    o: &mut Outcome,
    f: &Fixture,
    e: &Expect,
    link: &Value,
    named: &dyn Fn(&str) -> String,
    verified: bool,
) {
    let drafted = matches!(e.verdict.as_deref(), Some("inferred" | "ambiguous"));
    if !drafted {
        return;
    }
    let at = &e.global;
    o.that(link.get("map").is_some(), || {
        format!("{at}: no map drafted: {link}")
    });
    if verified {
        o.is(
            &format!("{at} compiled"),
            opt(&link["verified"], "compiled"),
            json!(true),
        );
        o.is(
            &format!("{at} row_found"),
            opt(&link["verified"], "row_found"),
            json!(true),
        );
    }
    if let Some(want) = declared_expressions(f, &e.global, named) {
        let got: Vec<Value> = link["map"]["subscripts"]
            .as_array()
            .map(|a| a.iter().map(|s| s["expression"].clone()).collect())
            .unwrap_or_default();
        let mut want: Vec<Value> = want.into_iter().map(Value::from).collect();
        // The ID in the node value is not a subscript.
        if e.id_form == "value" {
            want.retain(|x| x != "{ID}");
        }
        o.is(&format!("{at} drafted levels"), json!(got), json!(want));
    }
}

/// Every `<Expression>` of the fixture class's index map over `global`, in level order, with column
/// names spelt as the master draft names them and key parts as the seed's `key_map` says. `None`
/// when the class declares no map over that global (a class-compiled or hand-written index).
fn declared_expressions(
    f: &Fixture,
    global: &str,
    named: &dyn Fn(&str) -> String,
) -> Option<Vec<String>> {
    let after = f
        .cls_text
        .split(&format!("<Global>{global}</Global>"))
        .nth(1)?;
    let map = after.split("</SQLMap>").next()?;
    let mut out = Vec::new();
    for part in map.split("<Subscript name=").skip(1) {
        let mut e = part
            .split("<Expression>")
            .nth(1)?
            .split("</Expression>")
            .next()?
            .replace("&quot;", "\"");
        for c in f.master_columns().iter().filter(|c| !c.is_empty()) {
            e = e.replace(&format!("{{{c}}}"), &format!("{{{}}}", named(c)));
        }
        if let Some(keys) = f.seed.get("key_map").and_then(Value::as_object) {
            for (k, v) in keys {
                e = e.replace(&format!("{{{k}}}"), &format!("{{{}}}", v.as_str()?));
            }
        }
        out.push(e);
    }
    Some(out)
}

fn item_for(out: &Value, global: &str) -> Value {
    out["indexes"]
        .as_array()
        .and_then(|a| a.iter().find(|i| i["global"] == json!(global)))
        .cloned()
        .unwrap_or(Value::Null)
}

// ---- the drivers --------------------------------------------------------------------------------

async fn drive_link(r: &Rig, f: &Fixture, allow: &Rig) -> Outcome {
    let master = f.seed["master_global"].clone();
    let out = match propose(
        r,
        &f.name,
        json!({"global": master, "index_globals": f.seed["index_globals"]}),
    )
    .await
    {
        Ok(o) => o,
        Err(e) => {
            let mut o = Outcome::new(Value::Null);
            o.wrong.push(e);
            return o;
        }
    };
    let mut o = Outcome::new(out.clone());
    let named = |c: &str| draft_name(f, c);
    // F18's marker is a literal the sample alone cannot give: its draft is a guess until the caller
    // asks for values, so it is verified only in the run below.
    let verified = f.name != "F18";
    for e in expectations(&f.seed) {
        let link = item_for(&out, &e.global);
        if link.is_null() {
            o.wrong
                .push(format!("{}: no link in {}", e.global, out["indexes"]));
            continue;
        }
        check_link(&mut o, f, &e, &link, &named);
        check_draft(&mut o, f, &e, &link, &named, verified);
        independent_row(r, f, &e, &link, &out, &mut o).await;
        // The null marker's literal never leaves IRIS without the data policy and the call's say-so.
        o.that(
            link["subscripts"]
                .as_array()
                .into_iter()
                .flatten()
                .all(|s| s["null_marker"].get("literal").is_none()),
            || format!("{}: a literal in {link}", e.global),
        );
    }
    if let Some(n) = f.seed["answer"]
        .get("null")
        .filter(|n| n.get("literal").is_some())
    {
        values_run_f18(allow, f, n, &mut o).await;
    }
    if f.name == "F38" {
        // SC-005: two indexes over 1,200 rows inside five seconds, measured on a second call.
        let started = Instant::now();
        let again = ask_propose(
            r,
            json!({"global": f.seed["master_global"], "index_globals": f.seed["index_globals"]}),
        )
        .await;
        let took = started.elapsed();
        o.that(again["success"] == json!(true), || {
            format!("F38 repeat: {again}")
        });
        o.that(took < std::time::Duration::from_secs(5), || {
            format!("F38 took {took:?}, over the 5 s cap")
        });
    }
    if f.name == "F43" {
        uses_f43(r, f, &mut o).await;
    }
    o
}

/// SC-002, asked of IRIS and not of the draft's own `verified`: the class draft that carries the
/// inferred map compiles as a real class, and an equality on the indexed field returns the seed's
/// first row that holds a value. Where the draft says the planner uses the map, IRIS's own `EXPLAIN` names it.
/// Only a plain single-subscript ID master qualifies (no key map, no value ID, no killed rows).
async fn independent_row(
    r: &Rig,
    f: &Fixture,
    e: &Expect,
    link: &Value,
    out: &Value,
    o: &mut Outcome,
) {
    let a = &f.seed["answer"];
    let qualifies = e.verdict.as_deref() == Some("inferred")
        && e.id_form == "subscript"
        && e.id_positions
            .as_ref()
            .is_some_and(|p| p.as_array().is_some_and(|p| p.len() == 1))
        && f.seed.get("key_map").is_none()
        && a.get("killed_rows").is_none()
        && e.subs.first().is_some_and(|s| s["field"].is_string());
    if !qualifies || link.get("map").is_none() {
        return;
    }
    let column = e.subs[0]["field"].as_str().unwrap();
    let Some(at) = f.columns().iter().position(|c| c == column) else {
        return;
    };
    // The first row that holds a value in the field (a NULL has no equality), and its ID.
    let rows = f.rows();
    let Some(known) = rows.iter().position(|row| {
        matches!(&row[at], Value::String(s) if !s.is_empty()) || row[at].is_number()
    }) else {
        o.wrong
            .push(format!("{}: no row holds a {column}", e.global));
        return;
    };
    let known_id = (known + 1).to_string();
    let literal = match &rows[known][at] {
        Value::String(s) => format!("'{}'", s.replace('\'', "''")),
        other => other.to_string(),
    };
    let Some(source) = out["class_draft"]["source"].as_str() else {
        o.wrong
            .push(format!("{}: no class_draft.source to compile", e.global));
        return;
    };
    let Some(name) = source
        .strip_prefix("Class ")
        .and_then(|s| s.split(' ').next())
    else {
        o.wrong.push(format!(
            "{}: the class draft does not start with Class",
            e.global
        ));
        return;
    };
    let real = format!("IadSm173.{}Draft", f.name);
    let text = source.replacen(&format!("Class {name} "), &format!("Class {real} "), 1);
    let (compiled, why) = try_class(r, &real, &text).await;
    if !compiled {
        o.wrong.push(format!(
            "{}: the class draft does not compile: {why}",
            e.global
        ));
        return;
    }
    let field = draft_name(f, column);
    let query = format!("SELECT %ID AS id FROM {real} WHERE {field} = {literal}");
    let ids: Vec<String> = select(r, &query)
        .await
        .iter()
        .map(|row| row["id"].to_string().trim_matches('"').to_string())
        .collect();
    o.that(ids.contains(&known_id), || {
        format!("{}: {query} returned {ids:?}, not row {known_id}", e.global)
    });
    o.independent += 1;
    if link["verified"]["usability"]["status"] == json!("used") {
        let map = link["map"]["name"].as_str().unwrap_or_default();
        let planned = plan(r, &query).await;
        o.planned += 1;
        o.that(
            planned.contains(map) && planned.contains("index map"),
            || {
                format!(
                    "{}: the draft says the planner uses {map}, IRIS plans: {planned}",
                    e.global
                )
            },
        );
    }
    // The draft is a scratch class over the fixture's own globals: gone before the next check.
    let _ = r
        .call(
            "iris_doc",
            json!({"mode": "delete", "name": format!("{real}.cls"), "namespace": "USER"}),
        )
        .await;
}

/// F18 under `dataPolicy = allow` with `include_values` and `acknowledgePhi`: the literal is the
/// seed's, the draft carries it and verifies.
async fn values_run_f18(allow: &Rig, f: &Fixture, n: &Value, o: &mut Outcome) {
    let args = json!({"global": f.seed["master_global"], "index_globals": f.seed["index_globals"],
        "include_values": true, "acknowledgePhi": true});
    let out = ask_propose(allow, args).await;
    let link = item_for(&out, f.seed["answer"]["index_global"].as_str().unwrap());
    let position = n["position"].as_u64().unwrap();
    o.is(
        "F18 literal under include_values",
        opt(&sub_at(&link, position)["null_marker"], "literal"),
        n["literal"].clone(),
    );
    o.is(
        "F18 drafted NullMarker",
        opt(&link["map"]["subscripts"][0], "null_marker"),
        json!(format!("\"{}\"", n["literal"].as_str().unwrap())),
    );
    o.is(
        "F18 verified with the literal",
        opt(&link["verified"], "row_found"),
        json!(true),
    );
}

/// F43: the candidate is also another class's master; both classes still use it.
async fn uses_f43(r: &Rig, f: &Fixture, o: &mut Outcome) {
    o.is(
        "F43 found_by",
        opt(
            &item_for(
                &o.answer.clone(),
                f.seed["answer"]["index_global"].as_str().unwrap(),
            ),
            "found_by",
        ),
        json!("parameter"),
    );
    put_class(r, &f.class, &f.cls_text).await;
    let other = f.seed["answer"]["other_class"].as_str().unwrap();
    for extra in f.seed["extra_classes"].as_array().unwrap() {
        let text = std::fs::read_to_string(
            fixture_dir()
                .join("classes")
                .join(format!("{}.cls", extra.as_str().unwrap())),
        )
        .unwrap();
        put_class(r, other, &text).await;
    }
    let uses = r
        .call(
            "iris_storage_map",
            json!({"action": "uses", "global": f.seed["answer"]["index_global"], "namespace": "USER"}),
        )
        .await;
    let mut got: Vec<String> = uses["uses"]
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(|u| u["class"].as_str().map(String::from))
        .collect();
    got.sort();
    let mut want = vec![f.class.clone(), other.to_string()];
    want.sort();
    o.is("F43 uses", json!(got), json!(want));
}

async fn drive_sparse(r: &Rig, f: &Fixture, allow: &Rig) -> Outcome {
    let a = f.seed["answer"].clone();
    let field = draft_name(f, a.get("field").and_then(Value::as_str).unwrap_or("City"));
    let base =
        json!({"global": f.seed["master_global"], "index_globals": [f.seed["index_globals"][0]]});
    let fixed = a["rule"] == json!("fixed");
    // The fixed-value rule's literal is a sampled value: the first call asks for none.
    let out = match propose(if fixed { allow } else { r }, &f.name, base.clone()).await {
        Ok(o) => o,
        Err(e) => {
            let mut o = Outcome::new(Value::Null);
            o.wrong.push(e);
            return o;
        }
    };
    let mut o = Outcome::new(out.clone());
    sparse_checks(
        &mut o,
        f,
        &item_for(&out, a["index_global"].as_str().unwrap()),
        &a,
        &field,
        None,
    );
    if fixed {
        let mut with = base;
        with["include_values"] = json!(true);
        with["acknowledgePhi"] = json!(true);
        let out = ask_propose(allow, with).await;
        sparse_checks(
            &mut o,
            f,
            &item_for(&out, a["index_global"].as_str().unwrap()),
            &a,
            &field,
            Some(&a["literal"]),
        );
    }
    let why = o.answer["not_inferred_detail"].to_string();
    match a["rule"].as_str() {
        None => {
            o.that(
                why.contains("no rule fit") && why.contains("fewer rows"),
                || format!("not_inferred_detail lacks the rule's words: {why}"),
            );
        }
        Some("fixed") => {
            o.that(
                why.contains("include_values") && why.contains(&field),
                || format!("not_inferred_detail lacks the rule's words: {why}"),
            );
        }
        _ => {}
    }
    o
}

fn sparse_checks(
    o: &mut Outcome,
    f: &Fixture,
    link: &Value,
    a: &Value,
    field: &str,
    literal: Option<&Value>,
) {
    let m = &link["missing_rows"];
    let share = m["share"].as_f64().unwrap_or(-1.0);
    let want = a["missing_share"].as_f64().unwrap();
    o.that((share - want).abs() < 0.02, || {
        format!("share {share}, want {want}: {m}")
    });
    o.is("rows", opt(m, "rows"), a["rows"].clone());
    o.is(
        "missing",
        opt(m, "missing"),
        json!(a["missing_ids"].as_array().unwrap().len()),
    );
    o.is("verdict", opt(link, "verdict"), a["link_verdict"].clone());
    match a["rule"].as_str() {
        None => {
            o.is("rule", opt(m, "rule"), Value::Null);
            o.that(link["map"].get("condition").is_none(), || {
                format!("a condition for no rule: {link}")
            });
        }
        Some(kind) => {
            let mut rule = json!({"kind": kind, "field": field});
            if let Some(l) = literal {
                rule["literal"] = l.clone();
            }
            o.is("rule", opt(m, "rule"), rule);
            let drafted = kind != "fixed" || literal.is_some();
            if drafted {
                let cond = a["condition"].as_str().unwrap();
                let mut text = cond.to_string();
                for c in f.master_columns() {
                    text = text.replace(&format!("{{{c}}}"), &format!("{{{}}}", draft_name(f, &c)));
                }
                o.is("condition", opt(&link["map"], "condition"), json!(text));
                o.is(
                    "population_type",
                    opt(&link["map"], "population_type"),
                    json!("conditional"),
                );
                o.is(
                    "condition_fields",
                    opt(&link["map"], "condition_fields"),
                    json!(field),
                );
                let kept =
                    a["rows"].as_u64().unwrap() - a["missing_ids"].as_array().unwrap().len() as u64;
                o.is("compiled", opt(&link["verified"], "compiled"), json!(true));
                o.is(
                    "row_found",
                    opt(&link["verified"], "row_found"),
                    json!(true),
                );
                o.is(
                    "rows_kept",
                    opt(&link["verified"], "rows_kept"),
                    json!(kept),
                );
            } else {
                o.that(link["map"].get("condition").is_none(), || {
                    format!("a condition without values: {link}")
                });
            }
        }
    }
}

async fn drive_candidates(r: &Rig, f: &Fixture) -> Outcome {
    let out = match propose(r, &f.name, json!({"global": f.seed["master_global"]})).await {
        Ok(o) => o,
        Err(e) => {
            let mut o = Outcome::new(Value::Null);
            o.wrong.push(e);
            return o;
        }
    };
    let mut o = Outcome::new(out.clone());
    o.is(
        "listing",
        opt(&out["index_candidates"], "listing"),
        json!("ok"),
    );
    let named = |c: &str| draft_name(f, c);
    let want = expectations(&f.seed).remove(0);
    let cands = out["index_candidates"]["candidates"]
        .as_array()
        .cloned()
        .unwrap_or_default();
    let found = cands
        .iter()
        .find(|c| c["global"] == json!(want.global))
        .cloned()
        .unwrap_or(Value::Null);
    o.is("found_by", opt(&found, "found_by"), json!("name_prefix"));
    o.is("status", opt(&found, "status"), json!("examined"));
    let link = out["indexes"]
        .as_array()
        .and_then(|a| {
            a.iter().find(|l| {
                l["global"] == json!(want.global) && l["found_by"] == json!("name_prefix")
            })
        })
        .cloned()
        .unwrap_or(Value::Null);
    check_link(&mut o, f, &want, &link, &named);
    check_draft(&mut o, f, &want, &link, &named, true);
    o
}

async fn drive_sub_node(r: &Rig, f: &Fixture) -> Outcome {
    let out = match propose(r, &f.name, json!({"global": f.seed["master_global"]})).await {
        Ok(o) => o,
        Err(e) => {
            let mut o = Outcome::new(Value::Null);
            o.wrong.push(e);
            return o;
        }
    };
    let mut o = Outcome::new(out.clone());
    let master = f.seed["master_global"].as_str().unwrap();
    let prefix = f.seed["answer"]["sub_prefix"].clone();
    let found = out["index_candidates"]["candidates"]
        .as_array()
        .into_iter()
        .flatten()
        .find(|c| c["global"] == json!(master) && c["sub_prefix"] == prefix)
        .cloned()
        .unwrap_or(Value::Null);
    o.is("found_by", opt(&found, "found_by"), json!("sub_node"));
    o.is("status", opt(&found, "status"), json!("examined"));
    let link = out["indexes"]
        .as_array()
        .and_then(|a| {
            a.iter()
                .find(|l| l["global"] == json!(master) && l["found_by"] == json!("sub_node"))
        })
        .cloned()
        .unwrap_or(Value::Null);
    let mut want = expectations(&f.seed).remove(0);
    // The seed's `index_global` is the class's own map; the sub-node link is over the master.
    let x = want.global.clone();
    want.global = master.to_string();
    let named = |c: &str| draft_name(f, c);
    check_link(&mut o, f, &want, &link, &named);
    // The draft spells the literal level, then what the link found (read from the seed).
    let expr: Vec<Value> = link["map"]["subscripts"]
        .as_array()
        .into_iter()
        .flatten()
        .map(|s| s["expression"].clone())
        .collect();
    let mut levels: Vec<Value> = prefix
        .as_array()
        .unwrap()
        .iter()
        .map(|p| json!(format!("\"{}\"", p.as_str().unwrap())))
        .collect();
    levels.push(json!("$$SQLUPPER({Field1})"));
    levels.push(json!("{ID}"));
    o.is("drafted levels", json!(expr), json!(levels));
    o.is("compiled", opt(&link["verified"], "compiled"), json!(true));
    o.is(
        "row_found",
        opt(&link["verified"], "row_found"),
        json!(true),
    );
    // The class's own map is found by its name and links the same way.
    let by_name = out["indexes"]
        .as_array()
        .and_then(|a| {
            a.iter()
                .find(|l| l["global"] == json!(x) && l["found_by"] == json!("name_prefix"))
        })
        .cloned()
        .unwrap_or(Value::Null);
    o.is(
        "name_prefix link",
        opt(&by_name, "verdict"),
        json!("inferred"),
    );
    o
}

async fn drive_capped(r: &Rig, f: &Fixture) -> Outcome {
    let out = match propose(r, &f.name, json!({"global": f.seed["master_global"]})).await {
        Ok(o) => o,
        Err(e) => {
            let mut o = Outcome::new(Value::Null);
            o.wrong.push(e);
            return o;
        }
    };
    let mut o = Outcome::new(out.clone());
    let strings = |v: &Value| -> Vec<String> {
        v.as_array()
            .into_iter()
            .flatten()
            .filter_map(|x| x.as_str().map(String::from))
            .collect()
    };
    let a = &f.seed["answer"];
    let linked: Vec<String> = out["indexes"]
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(|l| l["global"].as_str().map(String::from))
        .collect();
    o.is("examined", json!(linked), json!(strings(&a["examined"])));
    o.is(
        "not_examined",
        json!(strings(&out["index_candidates"]["not_examined"])),
        json!(strings(&a["not_examined"])),
    );
    let listed: Vec<String> = out["index_candidates"]["candidates"]
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(|c| c["global"].as_str().map(String::from))
        .collect();
    o.is(
        "candidates",
        json!(listed),
        json!(strings(&a["candidates"])),
    );
    o
}

async fn load_keeping_class(r: &Rig, f: &Fixture) {
    put_class(r, &f.class, &f.cls_text).await;
    let columns = f.columns();
    let cols: Vec<&str> = columns.iter().map(String::as_str).collect();
    insert_rows(
        r,
        f.seed["table"].as_str().unwrap_or(&f.class),
        &cols,
        &f.rows(),
    )
    .await;
    if let Some(code) = f.seed["post_load"].as_str() {
        exec(r, code).await;
    }
}

async fn drive_class(r: &Rig, f: &Fixture, mapped: bool) -> Outcome {
    let out = match propose(r, &f.name, json!({"class": f.class})).await {
        Ok(o) => o,
        Err(e) => {
            let mut o = Outcome::new(Value::Null);
            o.wrong.push(e);
            return o;
        }
    };
    let mut o = Outcome::new(out.clone());
    o.is("class", opt(&out, "class"), json!(f.class));
    if mapped {
        let (global, map) = f.seed["answer"]["already_mapped"]
            .as_object()
            .unwrap()
            .iter()
            .next()
            .unwrap();
        let cand = out["index_candidates"]["candidates"]
            .as_array()
            .into_iter()
            .flatten()
            .find(|c| c["global"] == json!(global))
            .cloned()
            .unwrap_or(Value::Null);
        o.is("status", opt(&cand, "status"), json!("already_mapped"));
        o.is("map named", opt(&cand, "reason"), map.clone());
        o.that(out.get("indexes").is_none(), || {
            format!("something was drafted: {}", out["indexes"])
        });
        return o;
    }
    // The class's own names: `Name`, not `Field1`.
    let want = expectations(&f.seed).remove(0);
    let named = |c: &str| c.to_string();
    let link = out["indexes"][0].clone();
    check_link(&mut o, f, &want, &link, &named);
    let func = match want.subs[0]["transform"].as_str().unwrap() {
        "sqlupper" => "$$SQLUPPER",
        other => panic!("{}: no spelling for {other}", f.name),
    };
    let field = want.subs[0]["field"].as_str().unwrap();
    let levels: Vec<Value> = link["map"]["subscripts"]
        .as_array()
        .into_iter()
        .flatten()
        .map(|s| s["expression"].clone())
        .collect();
    o.is(
        "drafted levels",
        json!(levels),
        json!([format!("{func}({{{field}}})"), "{ID}"]),
    );
    o.is("compiled", opt(&link["verified"], "compiled"), json!(true));
    o.is(
        "row_found",
        opt(&link["verified"], "row_found"),
        json!(true),
    );
    o.that(!out.to_string().contains("Field1"), || {
        "a placeholder name reached the answer".into()
    });
    // `check` takes the draft as the class's new map and compiles the class.
    let checked = ask_check(r, json!({"class": f.class, "model": link["map"].clone()})).await;
    o.is("check compiled", opt(&checked, "compiled"), json!(true));
    o
}

/// `ids_by_status` of the seed against the bit strings the engine filed (bit position = ID + 1).
async fn bits_are_the_seed_ids(r: &Rig, f: &Fixture, o: &mut Outcome) {
    let global = f.seed["answer"]["index_global"].as_str().unwrap();
    let lines = dump_global(r, global).await;
    for (status, ids) in f.seed["answer"]["ids_by_status"]
        .as_object()
        .into_iter()
        .flatten()
    {
        let line = lines
            .iter()
            .find(|l| {
                // A class-compiled SQLUPPER subscript keeps a leading blank: `" CLOSED OUT"`.
                l.to_uppercase()
                    .contains(&format!("{}\",", status.to_uppercase()))
            })
            .cloned()
            .unwrap_or_default();
        let got: Vec<u64> = line
            .split_once("=<bits:")
            .map(|(_, b)| {
                b.trim_end_matches('>')
                    .split(',')
                    .filter_map(|p| p.parse().ok())
                    .collect()
            })
            .unwrap_or_default();
        let want: Vec<u64> = ids
            .as_array()
            .unwrap()
            .iter()
            .map(|i| i.as_u64().unwrap() + 1)
            .collect();
        o.is(&format!("bits of {status}"), json!(got), json!(want));
    }
}

async fn drive_bitmap(r: &Rig, f: &Fixture) -> Outcome {
    let global = f.seed["index_globals"][0].clone();
    let out = match propose(
        r,
        &f.name,
        json!({"global": f.seed["master_global"], "index_globals": [global]}),
    )
    .await
    {
        Ok(o) => o,
        Err(e) => {
            let mut o = Outcome::new(Value::Null);
            o.wrong.push(e);
            return o;
        }
    };
    let mut o = Outcome::new(out.clone());
    let a = f.seed["answer"].clone();
    if a.get("ids_by_status").is_some() {
        bits_are_the_seed_ids(r, f, &mut o).await;
    }
    let b = out["bitmaps"][0].clone();
    o.is(
        "bitmaps",
        json!(out["bitmaps"].as_array().map(Vec::len)),
        json!(1),
    );
    o.is("global", opt(&b, "global"), a["index_global"].clone());
    o.is(
        "chunk_position",
        opt(&b, "chunk_position"),
        a["chunk_position"].clone(),
    );
    o.is(
        "ids_found_in_master",
        opt(&b, "ids_found_in_master"),
        a["ids_found"].clone(),
    );
    o.is("verdict", opt(&b, "verdict"), a["link_verdict"].clone());
    o.is(
        "link item",
        opt(&out["indexes"][0], "verdict"),
        a["link_verdict"].clone(),
    );
    if let Some(n) = a.get("ids_checked") {
        o.is("ids_checked", opt(&b, "ids_checked"), n.clone());
    }
    if let Some(assumed) = a["assumed"].as_str() {
        o.that(b["assumed"].to_string().contains(assumed), || {
            format!("assumed {} lacks {assumed}", b["assumed"])
        });
        // The answer says how many of the IDs decoded from chunk 2 on found a row, in words.
        let counted = format!(
            "{} of the {} IDs",
            a["later_found"].as_u64().unwrap(),
            a["later_checked"].as_u64().unwrap()
        );
        o.that(b["assumed"].to_string().contains(&counted), || {
            format!("assumed {} lacks {counted:?}", b["assumed"])
        });
    }
    if a["no_map"] == json!(true) {
        o.that(b.get("map").is_none(), || {
            format!("a map for an unlinked bitmap: {b}")
        });
        return o;
    }
    // A tie on the chunk size (F19c) names no field: the seed holds the verdict and the counts.
    if a.get("field").is_none() {
        return o;
    }
    o.is("map type", opt(&b["map"], "type"), json!("bitmap"));
    // The field alone, after the index name when the class compiled it.
    let columns = f.master_columns();
    let at = columns
        .iter()
        .position(|c| c == a["field"].as_str().unwrap())
        .unwrap()
        + 1;
    let func = match a["transform"].as_str().unwrap() {
        "exact" => None,
        "sqlupper" => Some("$$SQLUPPER"),
        other => panic!("{}: no spelling for {other}", f.name),
    };
    let field = match func {
        Some(func) => format!("{func}({{Field{at}}})"),
        None => format!("{{Field{at}}}"),
    };
    let mut levels = Vec::new();
    if let Some(lead) = a["lead"].as_str() {
        levels.push(json!(format!("\"{lead}\"")));
    }
    levels.push(json!(field));
    let got: Vec<Value> = b["map"]["subscripts"]
        .as_array()
        .into_iter()
        .flatten()
        .map(|s| s["expression"].clone())
        .collect();
    o.is("map levels", json!(got), json!(levels));
    o.is("compiled", opt(&b["verified"], "compiled"), json!(true));
    o.is("row_found", opt(&b["verified"], "row_found"), json!(true));
    o
}

async fn drive_bit_master(r: &Rig) -> Outcome {
    let seed = read_seed("F21");
    exec(r, seed["post_load"].as_str().unwrap()).await;
    let out = match propose(r, "F21", json!({"global": seed["master_global"]})).await {
        Ok(o) => o,
        Err(e) => {
            let mut o = Outcome::new(Value::Null);
            o.wrong.push(e);
            return o;
        }
    };
    let mut o = Outcome::new(out.clone());
    let a = &seed["answer"];
    let draft = &out["chunk_map_draft"];
    let map = &draft["map"];
    o.is("map type", opt(map, "type"), json!("data"));
    o.is(
        "map global",
        opt(map, "global"),
        seed["master_global"].clone(),
    );
    o.is(
        "levels",
        json!(map["subscripts"].as_array().map(Vec::len)),
        json!(3),
    );
    o.is(
        "constant level",
        opt(&map["subscripts"][0], "expression"),
        a["constant"].clone(),
    );
    o.is(
        "computed level",
        opt(&map["subscripts"][2], "access_type"),
        json!("Other"),
    );
    o.is("compiled", opt(&draft["verified"], "compiled"), json!(true));
    o.is(
        "rows",
        opt(&draft["verified"], "rows"),
        a["id_count"].clone(),
    );
    o.is(
        "id_sum",
        opt(&draft["verified"], "id_sum"),
        a["id_sum"].clone(),
    );
    o
}

async fn drive_typing(r: &Rig, f: &Fixture) -> Outcome {
    let out = match propose(
        r,
        &f.name,
        json!({"global": f.seed["master_global"], "index_globals": f.seed["index_globals"], "field_names": f.columns()}),
    )
    .await
    {
        Ok(o) => o,
        Err(e) => {
            let mut o = Outcome::new(Value::Null);
            o.wrong.push(e);
            return o;
        }
    };
    let mut o = Outcome::new(out.clone());
    let a = &f.seed["answer"];
    for item in out["indexes"].as_array().into_iter().flatten() {
        o.is("index verdict", opt(item, "verdict"), json!("inferred"));
    }
    o.is(
        "compiled",
        opt(&out["class_draft"], "compiled"),
        json!(true),
    );
    for c in f.columns() {
        let p = out["class_draft"]["properties"]
            .as_array()
            .into_iter()
            .flatten()
            .find(|p| p["name"] == json!(c))
            .cloned()
            .unwrap_or(Value::Null);
        let want_type = a["types"][&c].as_str().unwrap();
        o.is(&format!("{c} type"), opt(&p, "type"), json!(want_type));
        let t = &a["typing"][&c];
        let want_evidence = match want_type {
            "%Integer" => json!({"integer": t["integer"], "of": t["of"]}),
            "%Numeric" => {
                json!({"numeric": t["numeric"], "max_scale": t["max_scale"], "of": t["of"]})
            }
            "%Date" => json!({"date_fit": t["date_fit"], "of": t["of"]}),
            _ => json!({"max_length": t["max_length"]}),
        };
        o.is(&format!("{c} evidence"), opt(&p, "evidence"), want_evidence);
        let params: Vec<String> = p["parameters"]
            .as_array()
            .into_iter()
            .flatten()
            .filter_map(|x| x.as_str().map(String::from))
            .collect();
        if want_type == "%Numeric" {
            let scale = format!("SCALE = {}", t["max_scale"]);
            o.that(params.contains(&scale), || {
                format!("{c}: {params:?} lack {scale}")
            });
        }
        let collation: Vec<&String> = params
            .iter()
            .filter(|x| x.starts_with("COLLATION"))
            .collect();
        match a["collations"].get(&c) {
            Some(Value::String(text)) => {
                o.is(&format!("{c} collation"), json!(collation), json!([text]))
            }
            Some(_) => o.that(collation.is_empty(), || format!("{c}: {collation:?}")),
            None => o.that(collation.is_empty(), || {
                format!("{c}: {collation:?} for an unlinked field")
            }),
        }
    }
    o
}

// ---- golden files -------------------------------------------------------------------------------

/// The answer without what changes on every run: the scratch class's name and the identity block.
fn stable(mut v: Value) -> Value {
    if let Some(o) = v.as_object_mut() {
        o.remove("scratch_class");
        o.remove("identity");
    }
    v
}

// ---- the sweep ----------------------------------------------------------------------------------

#[tokio::test]
#[ignore]
async fn every_fixture_gives_its_known_answer_and_matches_its_golden() {
    let names = seed_names();
    // SC-001: the corpus is at least 30 fixtures, and every seed has a driver (and the other way).
    assert!(
        names.len() >= 30,
        "only {} fixtures: {names:?}",
        names.len()
    );
    for n in &names {
        assert!(
            drive_of(n).is_some(),
            "seed {n} has no driver in the corpus table"
        );
    }
    for n in 1..=50 {
        let name = format!("F{n:02}");
        assert!(names.contains(&name), "{name} has no seed file");
    }

    let regen = std::env::var("IAD_REGEN_GOLDEN").is_ok_and(|v| v == "1");
    let golden_dir = fixture_dir().join("golden");
    if regen {
        std::fs::create_dir_all(&golden_dir).unwrap();
    }
    let r = live().await;
    let allow = rig_with(&Where::live(), Toolset::Merged, ALLOW).await;
    let started = Instant::now();
    let mut failures = Vec::new();
    let mut passed = 0usize;
    let (mut independent, mut planned) = (0usize, 0usize);
    for name in &names {
        let t = Instant::now();
        let drive = drive_of(name).unwrap();
        let outcome = if drive == Drive::BitMaster {
            drive_bit_master(&r).await
        } else {
            let f = Fixture::read(name);
            match drive {
                Drive::ClassNew | Drive::ClassMapped => load_keeping_class(&r, &f).await,
                _ => load_fixture_173(&r, &f).await,
            }
            let o = match drive {
                Drive::Link => drive_link(&r, &f, &allow).await,
                Drive::Sparse => drive_sparse(&r, &f, &allow).await,
                Drive::Candidates => drive_candidates(&r, &f).await,
                Drive::SubNode => drive_sub_node(&r, &f).await,
                Drive::Capped => drive_capped(&r, &f).await,
                Drive::ClassNew => drive_class(&r, &f, false).await,
                Drive::ClassMapped => drive_class(&r, &f, true).await,
                Drive::Bitmap => drive_bitmap(&r, &f).await,
                Drive::Typing => drive_typing(&r, &f).await,
                Drive::BitMaster => unreachable!(),
            };
            let left = drop_fixture_173(&r, &f).await;
            if !left.is_empty() {
                failures.push(format!("{name}: globals left after the drop: {left:?}"));
            }
            o
        };
        drop_all(&r).await;
        let mut outcome = outcome;

        // SC-003: nothing in a false-link fixture's answer is `inferred`.
        if FALSE_LINKS.contains(&name.as_str()) {
            let mut verdicts = Vec::new();
            collect_verdicts(&outcome.answer["indexes"], &mut verdicts);
            collect_verdicts(&outcome.answer["bitmaps"], &mut verdicts);
            let shown = outcome.answer["indexes"].to_string();
            outcome.that(!verdicts.iter().any(|v| v == "inferred"), || {
                format!("a false-link fixture was called inferred: {shown}")
            });
        }
        // Golden.
        if outcome.answer.is_object() {
            let path = golden_dir.join(format!("{name}.json"));
            let fresh =
                serde_json::to_string_pretty(&stable(outcome.answer.clone())).unwrap() + "\n";
            if regen {
                std::fs::write(&path, &fresh).unwrap();
            } else {
                match std::fs::read_to_string(&path) {
                    Ok(stored) if stored == fresh => {}
                    Ok(_) => outcome.wrong.push(format!(
                        "{} differs from the answer (IAD_REGEN_GOLDEN=1 rewrites it)",
                        path.display()
                    )),
                    Err(_) => outcome.wrong.push(format!(
                        "{} is missing (IAD_REGEN_GOLDEN=1 creates it)",
                        path.display()
                    )),
                }
            }
        }
        independent += outcome.independent;
        planned += outcome.planned;
        eprintln!(
            "corpus {name}: {:?} in {:?}, {} wrong",
            drive,
            t.elapsed(),
            outcome.wrong.len()
        );
        if outcome.wrong.is_empty() {
            passed += 1;
        } else {
            failures.push(format!("{name}: {}", outcome.wrong.join("\n    ")));
        }
    }
    eprintln!(
        "corpus: {passed} of {} fixtures right in {:?}; {independent} drafts compiled and queried \
         independently, {planned} of them planned by IRIS",
        names.len(),
        started.elapsed()
    );
    // A sweep that skips the independent check everywhere would still read green.
    assert!(
        independent >= 15 && planned >= 10,
        "SC-002: only {independent} drafts were queried independently and {planned} planned"
    );
    assert!(
        failures.is_empty(),
        "{passed} of {} right; the rest:\n{}",
        names.len(),
        failures.join("\n")
    );
    assert_eq!(
        passed,
        names.len(),
        "SC-001: every fixture gives its known answer"
    );
}

/// Every `verdict` string in `v`, at any depth.
fn collect_verdicts(v: &Value, out: &mut Vec<String>) {
    match v {
        Value::Object(o) => {
            for (k, x) in o {
                if k == "verdict" {
                    if let Some(s) = x.as_str() {
                        out.push(s.to_string());
                    }
                }
                collect_verdicts(x, out);
            }
        }
        Value::Array(a) => a.iter().for_each(|x| collect_verdicts(x, out)),
        _ => {}
    }
}
