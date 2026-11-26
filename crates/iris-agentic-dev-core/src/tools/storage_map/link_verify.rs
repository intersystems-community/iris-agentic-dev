//! Verifying a drafted index map (spec 173, FR-014, SC-002): the master draft and the drafted index
//! map go into a scratch class that carries every master field as a property, the scratch class is
//! compiled over the real globals (which writes nothing), and a query asks for a row by the indexed
//! field. Only a count comes back, so no value leaves IRIS.
//!
//! The indexed property carries the `COLLATION` the link found, so that SQL's own equality is the
//! one the index holds and the planner can use the map. A piece form (`$p($$SQLUPPER(...),",",1)`)
//! has no collation to carry; its query reads the master, and `detail` says so.

use super::link::{Link, MasterView, Transform};
use super::master::MasterSpec;
use super::model::map_node;
use super::tree::{emit_body, Node};
use super::typing::{class_source, collation_parameter, PropertySpec, PropertyType};
use super::write::{scratch_compile, scratch_probe};
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
}

impl Verification {
    pub fn to_json(&self) -> Value {
        let mut v = json!({"compiled": self.compiled, "row_found": self.row_found});
        if let Some(d) = &self.detail {
            v["detail"] = json!(d);
        }
        v
    }

    fn failed(compiled: bool, why: impl Into<String>) -> Self {
        Self {
            compiled,
            row_found: false,
            detail: Some(why.into()),
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
}

pub fn indexed_field(link: &Link, view: &MasterView) -> Result<Indexed, String> {
    let (sub, pick) = link
        .subscripts
        .iter()
        .find_map(|s| s.pick.as_ref().map(|p| (s, p)))
        .ok_or("no subscript of the link is explained")?;
    let field = usize::try_from(pick.field)
        .ok()
        .and_then(|n| n.checked_sub(1))
        .and_then(|i| view.field_names.get(i))
        .ok_or_else(|| {
            format!(
                "field {} is not one of the master's fields (position {})",
                pick.field, sub.position
            )
        })?
        .clone();
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
        });
    }
    let collation = collation_parameter(pick.transform.name(), pick.length)?;
    let note = (pick.transform == Transform::Truncate && pick.length.is_none())
        .then(|| "truncate has no length".to_string());
    Ok(Indexed {
        field,
        collation,
        note,
    })
}

/// The class text and its Storage nodes: one `%String` property per master field, the indexed one
/// carrying its collation, and a Storage block of the master draft and the index map.
pub fn class_text(
    master: &MasterSpec,
    master_map: &Value,
    index_map: &Value,
    indexed: &Indexed,
) -> Result<(String, Vec<Node>), String> {
    let properties: Vec<PropertySpec> = master
        .fields
        .iter()
        .map(|f| PropertySpec {
            name: f.name.clone(),
            ty: PropertyType {
                name: "%String".into(),
                parameters: Vec::new(),
                date_guess: None,
                evidence: json!({}),
                note: "A property for the scratch copy that verifies a drafted index map.".into(),
            },
            collation: (f.name == indexed.field)
                .then(|| indexed.collation.clone())
                .flatten(),
        })
        .collect();
    let draft = class_source("IrisDevTmp.SmDraft", &properties)?;
    let nodes = vec![
        Node {
            tag: "Type".into(),
            name: None,
            value: Some("%Storage.SQL".into()),
            children: None,
        },
        map_node(master_map)?,
        map_node(index_map)?,
    ];
    let body = draft
        .source
        .trim_end()
        .strip_suffix('}')
        .ok_or("the class draft does not end in a brace")?;
    let text = format!(
        "{body}Storage SQLStorage\n{{\n{}\n}}\n\n}}\n",
        emit_body(&super::tree::normalize(nodes.clone()))
    );
    Ok((text, nodes))
}

/// The query: how many rows share the first row's value of `field`. The first row is the one with
/// the lowest ID, so the answer is at least 1 when the index and the master agree, and 0 when the
/// index holds no key the equality looks up.
pub fn count_query(table: &str, field: &str) -> String {
    format!(
        "SELECT COUNT(*) AS n FROM {table} WHERE {field} = (SELECT TOP 1 {field} FROM {table} ORDER BY %ID)"
    )
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
    let indexed = match indexed_field(link, view) {
        Ok(i) => i,
        Err(why) => return Verification::failed(false, why),
    };
    if !is_plain_name(&indexed.field) {
        return Verification::failed(false, format!("{:?} is not a field name", indexed.field));
    }
    let (text, nodes) = match class_text(master, master_map, index_map, &indexed) {
        Ok(t) => t,
        Err(why) => return Verification::failed(false, why),
    };
    if let Err(refusal) = scratch_probe(&text, &nodes) {
        return Verification::failed(false, answer_text(&refusal));
    }
    let field = indexed.field.clone();
    let query = move |scratch: &str| count_query(scratch, &field);
    let compiled = scratch_compile(
        conn,
        client,
        &text,
        &nodes,
        ns,
        Some(&query),
        "no real class was touched",
    )
    .await;
    match compiled {
        Err(answer) => {
            let compiled = answer["error_code"] != "STORAGE_MAP_COMPILE_FAILED"
                && answer["error_code"] != "UPLOAD_FAILED"
                && answer["error_code"] != "STORAGE_MAP_INVALID";
            Verification::failed(compiled, answer_text(&answer))
        }
        Ok((_, None)) => Verification::failed(true, "the scratch copy compiled but no query ran"),
        Ok((_, Some(Err(e)))) => Verification::failed(true, format!("the query failed: {e}")),
        Ok((_, Some(Ok(answer)))) => {
            let n = answer["result"]["content"][0]
                .as_object()
                .and_then(|o| o.values().next())
                .and_then(|v| v.as_u64().or_else(|| v.as_str()?.parse().ok()));
            match n {
                Some(n) => Verification {
                    compiled: true,
                    row_found: n >= 1,
                    detail: if n >= 1 {
                        indexed.note
                    } else {
                        Some(
                            "the query by the indexed field found no row: the map's keys are not \
                             the ones the index holds"
                                .into(),
                        )
                    },
                },
                None => Verification::failed(true, "the query answered with no count"),
            }
        }
    }
}

fn is_plain_name(s: &str) -> bool {
    super::master::is_valid_property_name(s) && !s.starts_with('%')
}

/// The words of a refusal or error answer, with IRIS's compile errors when it has them.
fn answer_text(answer: &Value) -> String {
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
