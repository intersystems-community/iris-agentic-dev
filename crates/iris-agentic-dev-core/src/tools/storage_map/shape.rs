//! The shape of a global as the sampler saw it (data-model.md "Shape report"): what `compare`,
//! `preview` and `propose` set against a map.

use serde::Serialize;
use std::collections::BTreeMap;

/// The scan's limits.
#[derive(Debug, Clone, Default, Serialize, PartialEq, Eq)]
pub struct Cap {
    pub nodes: u32,
    pub seconds: u32,
}

/// The smallest and largest piece count seen.
#[derive(Debug, Clone, Copy, Default, Serialize, PartialEq, Eq)]
pub struct Pieces {
    pub min: u32,
    pub max: u32,
}

/// One value format seen at a depth: `node` is the quoted literal last subscript (`"contact"`)
/// when it is not a number, else `None` (the row node itself).
#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct Format {
    pub depth: u32,
    pub node: Option<String>,
    pub format: String,
    pub delimiter: Option<String>,
    pub pieces: Pieces,
}

#[derive(Debug, Clone, Default, Serialize, PartialEq)]
pub struct ShapeReport {
    pub global: String,
    pub nodes_scanned: u32,
    pub capped: bool,
    pub cap: Cap,
    /// Value-holding nodes per depth (0 is the global's root).
    pub depths: BTreeMap<String, u32>,
    /// Per depth, how many nodes had `$Data` 1, 10 and 11 (FR-020a).
    pub data_states: BTreeMap<String, BTreeMap<String, u32>>,
    /// Per depth, the literal (non-numeric) subscripts seen: sub-node names, never values.
    pub sub_nodes: BTreeMap<String, Vec<String>>,
    pub formats: Vec<Format>,
    /// Sample values, only with `include_values` after the data policy (FR-021).
    pub values: Option<Vec<serde_json::Value>>,
}

impl ShapeReport {
    /// Fold one sampled node into the report.
    pub(crate) fn add(&mut self, row: &serde_json::Value) {
        let depth = row["d"].as_u64().unwrap_or(0) as u32;
        let d = depth.to_string();
        let state = row["s"].as_u64().unwrap_or(0).to_string();
        *self
            .data_states
            .entry(d.clone())
            .or_default()
            .entry(state.clone())
            .or_default() += 1;
        if let Some(name) = row["n"].as_str() {
            let names = self.sub_nodes.entry(d.clone()).or_default();
            if !names.iter().any(|n| n == name) {
                names.push(name.to_string());
            }
        }
        let Some(format) = row["f"].as_str() else {
            // A node with children and no value ($Data 10) holds no format.
            return;
        };
        *self.depths.entry(d).or_default() += 1;
        let node = row["n"].as_str().map(|n| format!("\"{n}\""));
        let delimiter = row["dl"]
            .as_u64()
            .and_then(|c| char::from_u32(c as u32))
            .map(String::from);
        let pieces = row["p"].as_u64().unwrap_or(0) as u32;
        match self.formats.iter_mut().find(|f| {
            f.depth == depth && f.node == node && f.format == format && f.delimiter == delimiter
        }) {
            Some(f) => {
                f.pieces.min = f.pieces.min.min(pieces);
                f.pieces.max = f.pieces.max.max(pieces);
            }
            None => self.formats.push(Format {
                depth,
                node,
                format: format.to_string(),
                delimiter,
                pieces: Pieces {
                    min: pieces,
                    max: pieces,
                },
            }),
        }
        if let Some(v) = row.get("v") {
            self.values
                .get_or_insert_with(Vec::new)
                .push(serde_json::json!({
                    "depth": depth,
                    "node": row["n"],
                    "value": v,
                }));
        }
    }
}

/// One place where a map and the data disagree: the element's path, what the map says, what the
/// data shows, and the node it was seen at.
#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct Mismatch {
    pub path: String,
    pub expected: String,
    pub seen: String,
    pub node: Option<String>,
}

/// `"^"` → `^`: a map's delimiter as the character the data holds, when it is a quoted literal.
fn literal(v: &str) -> Option<String> {
    let t = v.trim();
    t.strip_prefix('"')
        .and_then(|r| r.strip_suffix('"'))
        .map(|s| s.replace("\"\"", "\""))
}

/// Where a data field's value sits: the row node's depth (`levels`), or one below it for a
/// `Node`. `None` for a computed node the sampler cannot place.
fn field_place(levels: u32, node: Option<&str>) -> Option<(u32, Option<String>)> {
    match node.map(str::trim) {
        None => Some((levels, None)),
        Some(n) if n.parse::<i64>().is_ok() => Some((levels + 1, None)),
        Some(n) if literal(n).is_some() => Some((levels + 1, Some(n.to_string()))),
        Some(_) => None,
    }
}

/// The mismatches between data map `map` and what the sampler saw (US5, FR-020, FR-020a).
pub fn compare_map(map: &super::model::SqlMap, shape: &ShapeReport) -> Vec<Mismatch> {
    let mut out = Vec::new();
    let levels = map.subscripts.len() as u32;
    let at = format!("/maps/{}", map.name);
    let mut reads_row_node = false;
    let mut read_sub_nodes: Vec<String> = Vec::new();
    for (name, field) in &map.data {
        if field.retrieval_code.is_some() {
            continue; // computed: not read from the global
        }
        let path = format!("{at}/data/{name}");
        let Some((depth, node)) = field_place(levels, field.node.as_deref()) else {
            continue;
        };
        match &node {
            None if depth == levels => reads_row_node = true,
            Some(n) => read_sub_nodes.push(n.clone()),
            None => {}
        }
        let seen: Vec<&Format> = shape
            .formats
            .iter()
            .filter(|f| f.depth == depth && f.node == node)
            .collect();
        if seen.is_empty() {
            out.push(Mismatch {
                path,
                expected: format!("a value at depth {depth}"),
                seen: "no value there".into(),
                node,
            });
            continue;
        }
        let piece: u32 = field
            .piece
            .as_deref()
            .and_then(|p| p.trim().parse().ok())
            .unwrap_or(1);
        let want = field
            .delimiter
            .as_deref()
            .map(|d| literal(d).unwrap_or_default());
        for f in seen {
            let problem = match (&want, f.format.as_str()) {
                (Some(d), "delimited") if f.delimiter.as_deref() != Some(d.as_str()) => Some((
                    format!("pieces delimited by {d:?}"),
                    format!(
                        "delimited by {:?}",
                        f.delimiter.as_deref().unwrap_or_default()
                    ),
                )),
                (Some(d), "$LIST") => Some((
                    format!("pieces delimited by {d:?}"),
                    "$LIST values".to_string(),
                )),
                (None, "delimited") => Some((
                    "$LIST (no delimiter)".to_string(),
                    format!(
                        "text delimited by {:?}",
                        f.delimiter.as_deref().unwrap_or_default()
                    ),
                )),
                (None, "plain") if f.pieces.max > 0 => {
                    Some(("$LIST (no delimiter)".to_string(), "plain text".to_string()))
                }
                _ if piece > f.pieces.max.max(1) => Some((
                    format!("piece {piece}"),
                    format!("at most {} piece(s)", f.pieces.max),
                )),
                _ => None,
            };
            if let Some((expected, seen)) = problem {
                out.push(Mismatch {
                    path: path.clone(),
                    expected,
                    seen,
                    node: node.clone(),
                });
            }
        }
    }
    // FR-020a: what sits on the row node or under it that no field reads.
    let below = (levels + 1).to_string();
    for name in shape.sub_nodes.get(&below).into_iter().flatten() {
        let quoted = format!("\"{name}\"");
        if !read_sub_nodes.contains(&quoted) {
            out.push(Mismatch {
                path: at.clone(),
                expected: "the map reads every node under the row node".into(),
                seen: "row node has children the map does not read".into(),
                node: Some(quoted),
            });
        }
    }
    let row_values = shape.depths.get(&levels.to_string()).copied().unwrap_or(0);
    if levels > 0 && row_values > 0 && !reads_row_node {
        out.push(Mismatch {
            path: at,
            expected: "fields on the row node".into(),
            seen: "the row node holds a value the map does not read".into(),
            node: None,
        });
    }
    out
}
