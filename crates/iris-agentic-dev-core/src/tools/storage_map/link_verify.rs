//! Verifying a drafted index map (spec 173, FR-014, SC-002): the master draft and the drafted index
//! map go into a scratch class that carries every master field as a property, the scratch class is
//! compiled over the real globals (which writes nothing), and a query asks for a row by the indexed
//! field. Only a count comes back, so no value leaves IRIS.
//!
//! The indexed property carries the `COLLATION` the link found, so that SQL's own equality is the
//! one the index holds and the planner can use the map. A piece form (`$p($$SQLUPPER(...),",",1)`)
//! has no collation to carry; its query reads the master, and `detail` says so.

use super::link::{master_field, storage_type_node, Condition, Link, MasterView, Transform};
use super::master::MasterSpec;
use super::model::map_node;
use super::tree::{emit_body, Node};
use super::typing::{class_source, collation_parameter, PropertySpec, PropertyType};
use super::usability::{explain_prefix_query, line_for, Line, Question};
use super::usability_run::plan_text;
use super::write::{scratch_compile_queries, scratch_probe};
use crate::iris::connection::IrisConnection;
use serde_json::{json, Value};

/// What the scratch copy showed.
#[derive(Debug, Clone, PartialEq)]
pub struct Verification {
    /// The scratch class with the drafted map compiled.
    pub compiled: bool,
    /// A query by the indexed field found the row the master's first ID names.
    pub row_found: bool,
    /// Why not, or what the query could not show.
    pub detail: Option<String>,
    /// The planner's own line for an equality on the field the draft links, asked in the same
    /// scratch copy (SC-006). `None` when no property collation spells the form, so the planner
    /// cannot be asked.
    pub usability: Option<Line>,
}

impl Verification {
    pub fn to_json(&self) -> Value {
        let mut v = json!({"compiled": self.compiled, "row_found": self.row_found});
        if let Some(d) = &self.detail {
            v["detail"] = json!(d);
        }
        if let Some(l) = &self.usability {
            v["usability"] = l.to_json();
        }
        v
    }

    /// The draft compiled, found its row, and (where the planner can be asked) the planner reads
    /// its map for the field it links.
    pub fn verified(&self) -> bool {
        self.compiled
            && self.row_found
            && self.usability.as_ref().is_none_or(|l| l.status == "used")
    }

    /// Why [`Self::verified`] is false.
    pub fn not_verified_because(&self) -> String {
        match &self.usability {
            Some(l) if self.compiled && self.row_found && l.status != "used" => format!(
                "the draft compiled and found its row, but the planner does not use the map for \
                 an equality on {} ({}): {}",
                l.field, l.status, l.text
            ),
            _ => self
                .detail
                .clone()
                .unwrap_or_else(|| "no reason given".into()),
        }
    }

    fn failed(compiled: bool, why: impl Into<String>) -> Self {
        Self {
            compiled,
            row_found: false,
            detail: Some(why.into()),
            usability: None,
        }
    }
}

/// The field a link's first explained subscript reads, with the `COLLATION` parameter that makes
/// SQL compare the way the index files it (`None` for the default collation or for a form no
/// collation spells), and a note when the query cannot reach the index.
pub struct Indexed {
    pub field: String,
    pub collation: Option<String>,
    pub note: Option<String>,
    /// The map subscript the field is read at.
    pub position: u32,
    /// The index holds one piece of a collated value: no equality query reaches it.
    pub piece: bool,
}

pub fn indexed_field(link: &Link, view: &MasterView) -> Result<Indexed, String> {
    let (sub, pick) = link
        .subscripts
        .iter()
        .find_map(|s| s.pick.as_ref().map(|p| (s, p)))
        .ok_or("no subscript of the link is explained")?;
    let field = master_field(view, pick.field)
        .map_err(|_| {
            format!(
                "field {} is not one of the master's fields (position {})",
                pick.field, sub.position
            )
        })?
        .to_string();
    if pick.piece.is_some() {
        return Ok(Indexed {
            field,
            collation: None,
            note: Some(
                "the index holds one piece of a collated value, which no property collation \
                 spells, so the query read the master; `check` tells whether the planner uses \
                 the index"
                    .into(),
            ),
            position: sub.position,
            piece: true,
        });
    }
    let collation = collation_parameter(pick.transform.name(), pick.length)?;
    let note = (pick.transform == Transform::Truncate && pick.length.is_none())
        .then(|| "truncate has no length".to_string());
    Ok(Indexed {
        field,
        collation,
        note,
        position: sub.position,
        piece: false,
    })
}

/// The key-part names a scratch class gives a composite row ID: the master's own, with the
/// placeholder `ID` (the last part of a draft's composite key) renamed `Key<k>`, because `{ID}`
/// is the system ID of a single-level master and no property can carry it.
fn scratch_key_names(master: &MasterSpec) -> Result<Vec<String>, String> {
    let k = master.key_names.len();
    let names: Vec<String> = master
        .key_names
        .iter()
        .map(|n| {
            if n == "ID" {
                format!("Key{k}")
            } else {
                n.clone()
            }
        })
        .collect();
    for (i, n) in names.iter().enumerate() {
        let clash = master.fields.iter().any(|f| f.name.eq_ignore_ascii_case(n))
            || names[..i].iter().any(|m| m.eq_ignore_ascii_case(n));
        if clash || !is_plain_name(n) {
            return Err(format!(
                "the key part {n:?} cannot be a property of the scratch copy"
            ));
        }
    }
    Ok(names)
}

/// `map` with `{ID}` in its subscript expressions renamed to `{to}`.
fn rename_id(map: &Value, to: &str) -> Value {
    let mut out = map.clone();
    if let Some(subs) = out["subscripts"].as_array_mut() {
        for s in subs {
            if let Some(e) = s["expression"].as_str() {
                s["expression"] = json!(e.replace("{ID}", &format!("{{{to}}}")));
            }
        }
    }
    out
}

/// The class text and its Storage nodes: one `%String` property per master field, the indexed one
/// carrying its collation, and a Storage block of the master draft and the index map. A composite
/// row ID (k key parts) adds one `%String` property per part, an `IDKEY` index over them and the
/// `RowIdSpec` the master map then needs.
pub fn class_text(
    master: &MasterSpec,
    master_map: &Value,
    index_map: &Value,
    indexed: &Indexed,
) -> Result<(String, Vec<Node>), String> {
    let composite = master.key_names.len() > 1;
    let keys = if composite {
        scratch_key_names(master)?
    } else {
        Vec::new()
    };
    let property = |name: &str, collation: Option<String>| PropertySpec {
        name: name.to_string(),
        ty: PropertyType {
            name: "%String".into(),
            parameters: Vec::new(),
            date_guess: None,
            evidence: json!({}),
            note: "A property for the scratch copy that verifies a drafted index map.".into(),
        },
        collation,
    };
    let mut properties: Vec<PropertySpec> = keys.iter().map(|k| property(k, None)).collect();
    properties.extend(master.fields.iter().map(|f| {
        property(
            &f.name,
            (f.name == indexed.field)
                .then(|| indexed.collation.clone())
                .flatten(),
        )
    }));
    let draft = class_source("IrisDevTmp.SmDraft", &properties)?;
    let (master_map, index_map) = match keys.last() {
        Some(last) => {
            let mut m = rename_id(master_map, last);
            let row_id: Vec<Value> = keys
                .iter()
                .enumerate()
                .map(|(i, k)| json!({"position": i + 1, "expression": format!("{{L{}}}", i + 1), "field": k}))
                .collect();
            m["row_id"] = json!(row_id);
            (m, rename_id(index_map, last))
        }
        None => (master_map.clone(), index_map.clone()),
    };
    let nodes = vec![
        storage_type_node(),
        map_node(&master_map)?,
        map_node(&index_map)?,
    ];
    let body = draft
        .source
        .trim_end()
        .strip_suffix('}')
        .ok_or("the class draft does not end in a brace")?;
    let idkey = if composite {
        format!(
            "Index IDKEY On ({}) [ IdKey, Unique ];\n\n",
            keys.join(", ")
        )
    } else {
        String::new()
    };
    let text = format!(
        "{body}{idkey}Storage SQLStorage\n{{\n{}\n}}\n\n}}\n",
        emit_body(&super::tree::normalize(nodes.clone()))
    );
    Ok((text, nodes))
}

/// The query: how many rows share the first non-empty value of `field`. The first row is the one
/// with the lowest ID that has a value (an empty one is the index's null marker, which an equality
/// never looks up), so the answer is at least 1 when the index and the master agree, and 0 when the
/// index holds no key the equality looks up.
pub fn count_query(table: &str, field: &str) -> String {
    format!(
        "SELECT COUNT(*) AS n FROM {table} WHERE {field} = (SELECT TOP 1 {field} FROM {table} WHERE {field} IS NOT NULL AND {field} <> '' ORDER BY %ID)"
    )
}

/// The query that asks the master what a condition keeps: how many of the first `rows` rows (by ID,
/// the order the linker read them in) satisfy it, over the scratch table `table`. The caller has
/// checked `field` is a plain property name.
pub fn present_count_query(table: &str, field: &str, cond: &Condition, rows: u32) -> String {
    format!(
        "SELECT COUNT(*) AS n FROM (SELECT TOP {rows} %ID AS rid, {field} AS fv FROM {table} ORDER BY %ID) AS q WHERE {}",
        cond.sql_present("fv")
    )
}

/// What a conditional index map must hold: the master keeps `expect` of its first `rows` rows under
/// the condition, which is the number of rows the index has nodes for.
pub struct KeptCheck<'a> {
    pub condition: &'a Condition,
    pub rows: u32,
    pub expect: u64,
}

/// Compile the drafted map in a scratch copy and ask for a known row by the indexed field.
#[allow(clippy::too_many_arguments)] // the call site is the one in link_run.rs and the live test
pub async fn verify(
    conn: &IrisConnection,
    client: &reqwest::Client,
    ns: &str,
    master: &MasterSpec,
    master_map: &Value,
    index_map: &Value,
    link: &Link,
    view: &MasterView,
) -> Verification {
    verify_with(
        conn, client, ns, master, master_map, index_map, link, view, None,
    )
    .await
    .0
}

/// [`verify`], or, with a [`KeptCheck`], the check of a conditional map: the scratch copy compiles,
/// and the master keeps exactly the rows the index holds under the condition. The second item is the
/// count the master kept.
#[allow(clippy::too_many_arguments)] // the call site is the one in link_run.rs
pub async fn verify_with(
    conn: &IrisConnection,
    client: &reqwest::Client,
    ns: &str,
    master: &MasterSpec,
    master_map: &Value,
    index_map: &Value,
    link: &Link,
    view: &MasterView,
    kept: Option<&KeptCheck<'_>>,
) -> (Verification, Option<u64>) {
    let fail = |compiled: bool, why: String| (Verification::failed(compiled, why), None);
    let indexed = match indexed_field(link, view) {
        Ok(i) => i,
        Err(why) => return fail(false, why),
    };
    if !is_plain_name(&indexed.field) {
        return fail(false, format!("{:?} is not a field name", indexed.field));
    }
    if let Some(k) = kept {
        if !is_plain_name(&k.condition.field) {
            return fail(
                false,
                format!("{:?} is not a field name", k.condition.field),
            );
        }
    }
    let (text, nodes) = match class_text(master, master_map, index_map, &indexed) {
        Ok(t) => t,
        Err(why) => return fail(false, why),
    };
    if let Err(refusal) = scratch_probe(&text, &nodes) {
        return fail(false, answer_text(&refusal));
    }
    let field = indexed.field.clone();
    // The count (or, for a conditional map, what the master keeps under the condition), and (when
    // a property collation spells the form) the planner's plan for an equality on the same field,
    // all asked while the scratch copy exists.
    let ask_planner = !indexed.piece;
    let kept_check = kept.map(|k| (k.condition.clone(), k.rows));
    let queries = move |scratch: &str| {
        let mut q = vec![match &kept_check {
            Some((cond, rows)) => present_count_query(scratch, &cond.field, cond, *rows),
            None => count_query(scratch, &field),
        }];
        if ask_planner {
            q.push(explain_prefix_query(scratch, std::slice::from_ref(&field)).unwrap_or_default());
        }
        q
    };
    let compiled = scratch_compile_queries(
        conn,
        client,
        &text,
        &nodes,
        ns,
        Some(&queries),
        "no real class was touched",
    )
    .await;
    let compiled = compiled.map(|(scratch, answers)| {
        let mut answers = answers.into_iter().flatten();
        let count = answers.next();
        let plan = answers.next();
        let line = plan.map(|plan| {
            let question = Question {
                map: index_map["name"].as_str().unwrap_or_default().to_string(),
                level: indexed.position,
                field: indexed.field.clone(),
                fields: vec![indexed.field.clone()],
                unanswerable: None,
            };
            let plan = plan.map_err(|e| e.to_string()).and_then(|a| plan_text(&a));
            line_for(
                &question,
                plan.as_deref().map_err(String::as_str),
                &scratch,
                "<class>",
            )
        });
        (scratch, count, line)
    });
    match compiled {
        Err(answer) => {
            let compiled = !matches!(
                answer["error_code"].as_str(),
                Some("STORAGE_MAP_COMPILE_FAILED" | "UPLOAD_FAILED" | "STORAGE_MAP_INVALID")
            );
            fail(compiled, answer_text(&answer))
        }
        Ok((_, None, _)) => fail(true, "the scratch copy compiled but no query ran".into()),
        Ok((_, Some(Err(e)), _)) => fail(true, format!("the query failed: {e}")),
        Ok((_, Some(Ok(answer)), usability)) => {
            let n = answer["result"]["content"][0]
                .as_object()
                .and_then(|o| o.values().next())
                .and_then(|v| v.as_u64().or_else(|| v.as_str()?.parse().ok()));
            match (n, kept) {
                (None, _) => fail(true, "the query answered with no count".into()),
                (Some(n), Some(k)) if n != k.expect || n == 0 => (
                    Verification::failed(
                        true,
                        format!(
                            "the master keeps {n} of its first {} rows under {} and the index \
                             holds {}: the condition is not the one the index was filed under",
                            k.rows, k.condition.expression, k.expect
                        ),
                    ),
                    Some(n),
                ),
                (Some(0), None) => fail(
                    true,
                    "the query by the indexed field found no row: the map's keys are not the \
                     ones the index holds"
                        .into(),
                ),
                (Some(n), _) => (
                    Verification {
                        compiled: true,
                        row_found: true,
                        detail: indexed.note,
                        usability,
                    },
                    Some(n),
                ),
            }
        }
    }
}

fn is_plain_name(s: &str) -> bool {
    super::master::is_valid_property_name(s) && !s.starts_with('%')
}

/// The words of a refusal or error answer, with IRIS's compile errors when it has them.
pub(super) fn answer_text(answer: &Value) -> String {
    let mut text = answer["error"]
        .as_str()
        .or_else(|| answer["message"].as_str())
        .unwrap_or("refused")
        .to_string();
    if let Some(errors) = answer["iris_errors"].as_array() {
        let shown: Vec<String> = errors
            .iter()
            .take(3)
            .map(|e| e.as_str().map_or_else(|| e.to_string(), String::from))
            .collect();
        text.push_str(&format!(": {}", shown.join("; ")));
    }
    text
}
