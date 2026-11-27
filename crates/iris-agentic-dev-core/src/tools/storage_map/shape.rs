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
    /// Per depth, how many nodes had a numeric last subscript.
    pub numeric_subscripts: BTreeMap<String, u32>,
    pub formats: Vec<Format>,
    /// Sample values, only with `include_values` after the data policy (FR-021).
    pub values: Option<Vec<serde_json::Value>>,
}

impl ShapeReport {
    /// The depths (first subscript is depth 1) whose literal subscripts are the data's keys and
    /// not its structure, with how many distinct keys each holds. A sub-node name (`"title"`
    /// under every row) repeats once per row; the keys of an index held inside the master
    /// (`^M("idx","SMITH, ANNA",7)`) each occur once. Three or more distinct names that none
    /// repeats, at depth 2 or below, are keys: fewer cannot be told from the names of a
    /// one-row global, and depth 1 holds the names of whole sub-trees (`candidates` reads
    /// them), which are structure.
    fn keyed_depths(&self) -> Vec<(String, usize)> {
        self.sub_nodes
            .iter()
            .filter(|(d, names)| {
                let rows: u32 = self.data_states.get(*d).map_or(0, |s| s.values().sum());
                let numeric = self.numeric_subscripts.get(*d).copied().unwrap_or(0);
                d.parse::<u32>().is_ok_and(|d| d >= 2)
                    && names.len() >= 3
                    && rows.saturating_sub(numeric) as usize == names.len()
            })
            .map(|(d, names)| (d.clone(), names.len()))
            .collect()
    }

    /// The report as an answer carries it. Without `include_values` the keys that [`keyed_depths`]
    /// finds are values the sampler read, so their names stay in IRIS: `sub_nodes` loses the
    /// depth, `withheld_sub_nodes` counts what it held, and a format named after a key loses
    /// its node name. The report itself is not changed: `write` and `candidates` read the keys
    /// inside iad.
    ///
    /// [`keyed_depths`]: Self::keyed_depths
    pub fn for_answer(&self, include_values: bool) -> serde_json::Value {
        let mut answer = serde_json::json!(self);
        let keyed = if include_values {
            Vec::new()
        } else {
            self.keyed_depths()
        };
        if keyed.is_empty() {
            return answer;
        }
        let mut withheld = serde_json::Map::new();
        for (depth, count) in &keyed {
            if let Some(names) = answer["sub_nodes"].as_object_mut() {
                names.remove(depth);
            }
            withheld.insert(depth.clone(), serde_json::json!(count));
        }
        answer["withheld_sub_nodes"] = serde_json::Value::Object(withheld);
        let mut merged: Vec<Format> = Vec::new();
        for f in &self.formats {
            let mut f = f.clone();
            if keyed.iter().any(|(d, _)| *d == f.depth.to_string()) {
                f.node = None;
            }
            match merged.iter_mut().find(|m| {
                m.depth == f.depth
                    && m.node == f.node
                    && m.format == f.format
                    && m.delimiter == f.delimiter
            }) {
                Some(m) => {
                    m.pieces.min = m.pieces.min.min(f.pieces.min);
                    m.pieces.max = m.pieces.max.max(f.pieces.max);
                }
                None => merged.push(f),
            }
        }
        answer["formats"] = serde_json::json!(merged);
        answer
    }

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
        if depth > 0 && row["n"].is_null() {
            *self.numeric_subscripts.entry(d.clone()).or_default() += 1;
        }
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

/// What `compare` could and could not check, beside its mismatches.
#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct CompareNotes {
    /// Whether any data was sampled. `false` means "no mismatches" says nothing about the map.
    pub verified: bool,
    pub notes: Vec<String>,
}

/// The levels of `map` its own code computes (AccessType Other, NextCode, an Invalidcondition),
/// which need not be subscripts of the global.
fn computed_levels(map: &super::model::SqlMap) -> Vec<u32> {
    map.subscripts
        .iter()
        .filter(|s| {
            !super::model::computed_reasons(s).is_empty() || !s.invalid_conditions.is_empty()
        })
        .map(|s| s.level)
        .collect()
}

/// `compare`'s honesty notes (Benjamin De Boe's iFind case): an empty sample is "not checked", not
/// "no mismatches", and a map with computed levels is set against the depth the data really has.
pub fn compare_notes(map: &super::model::SqlMap, report: &ShapeReport) -> CompareNotes {
    if report.nodes_scanned == 0 {
        return CompareNotes {
            verified: false,
            notes: vec![format!(
                "no data found at {}, the map was not checked",
                report.global
            )],
        };
    }
    let computed = computed_levels(map);
    let declared = map.subscripts.len() as u32;
    let observed = report
        .depths
        .iter()
        .filter_map(|(d, n)| Some((d.parse::<u32>().ok()?, *n)))
        .filter(|&(d, _)| d != 0)
        .max_by_key(|&(d, n)| (n, std::cmp::Reverse(d)))
        .map(|(d, _)| d);
    let mut notes = Vec::new();
    if let Some(observed) = observed.filter(|&d| !computed.is_empty() && d != declared) {
        let levels = computed
            .iter()
            .map(u32::to_string)
            .collect::<Vec<_>>()
            .join(", ");
        notes.push(format!(
            "the map declares {declared} levels, of which level {levels} is computed by code; the data sits at depth {observed}, so a computed level is not a subscript of the global and the row node is not where the map's subscript count says"
        ));
    }
    CompareNotes {
        verified: true,
        notes,
    }
}

/// One thing `propose` leaves to the caller: the line `not_inferred` carries, and the same item as
/// `what` and `why` for `not_inferred_detail` (the line starts with `what`).
type NotInferred = (&'static str, &'static str, &'static str);

const NOT_INFERRED: [NotInferred; 7] = [
    (
        "field names (Field1, Field2, ... are placeholders; rename them to the class's properties)",
        "field names",
        "Field1, Field2, ... are placeholders; rename them to the class's properties",
    ),
    (
        "datatypes (they belong to the class's properties, not the map)",
        "datatypes",
        "they belong to the class's properties, not the map",
    ),
    (
        "collations (whether the class compares text case-sensitively)",
        "collations",
        "whether the class compares text case-sensitively",
    ),
    (
        "index maps (an index global such as ^X.x is not linked to this map)",
        "index maps",
        "an index global such as ^X.x is not linked to this map unless it is named in index_globals",
    ),
    (
        "conditions (a conditional map or an index that skips rows)",
        "conditions",
        "a conditional map or an index that skips rows",
    ),
    (
        "computed subscripts (levels produced by code, such as a $BIT chunk)",
        "computed subscripts",
        "levels produced by code, such as a $BIT chunk",
    ),
    (
        "the ID counter (SqlIdExpression) and any row ID built from several parts",
        "the ID counter",
        "SqlIdExpression is not read, and a row ID built from several parts is not inferred",
    ),
];

/// What a call that links index globals adds (FR-032): what the data could mean and the tool does
/// not draft.
const NOT_INFERRED_INDEX: [NotInferred; 3] = [
    (
        "COLLATE (a subscript collated by %COLLATE is recognised and never drafted)",
        "COLLATE",
        "a subscript collated by %COLLATE is recognised and never drafted",
    ),
    (
        "numeric collations (PLUS and MINUS are not map-expression functions, so a subscript they \
         produce is not drafted)",
        "numeric collations",
        "PLUS and MINUS are not map-expression functions, so a subscript they produce is not drafted",
    ),
    (
        "computed subscripts in an index (a subscript made by code, such as a date format, is \
         reported as not explained and no code is guessed)",
        "computed subscripts in an index",
        "a subscript made by code, such as a date format, is reported as not explained and no code \
         is guessed",
    ),
];

/// What `propose` says when the sampler saw bit-string values (format `bits`): the depths that
/// hold them and that no field is drafted from a bit string. `None` when none were seen.
pub fn bit_string_note(report: &ShapeReport) -> Option<String> {
    let mut depths: Vec<u32> = report
        .formats
        .iter()
        .filter(|f| f.format == "bits")
        .map(|f| f.depth)
        .collect();
    depths.sort_unstable();
    depths.dedup();
    if depths.is_empty() {
        return None;
    }
    let at = depths
        .iter()
        .map(u32::to_string)
        .collect::<Vec<_>>()
        .join(", ");
    Some(format!(
        "the values at depth {at} are bit strings ($BIT), not delimited fields, so no field is \
         drafted from them; a bit-string master is mapped by chunk_map_draft and a bit-string \
         index global is linked by bitmaps, where either applies"
    ))
}

/// What `propose` leaves to the caller, always returned so a draft is never read as complete.
pub fn not_inferred() -> Vec<String> {
    NOT_INFERRED.iter().map(|i| i.0.to_string()).collect()
}

/// `not_inferred` (strings) and `not_inferred_detail` (`{what, why}`) for one call: the 172 list,
/// then, when the call linked index globals, what FR-032 names, then `extra` (what this call did
/// not examine or explain, each `{what, why}`, which also reads as `what (why)`). One list builds
/// both, so every string has its detail item.
pub fn not_inferred_lists(
    extra: &[serde_json::Value],
    index_call: bool,
) -> (Vec<String>, Vec<serde_json::Value>) {
    let fixed = NOT_INFERRED
        .iter()
        .chain(NOT_INFERRED_INDEX.iter().filter(|_| index_call));
    let mut strings: Vec<String> = fixed.clone().map(|i| i.0.to_string()).collect();
    let mut details: Vec<serde_json::Value> = fixed
        .map(|i| serde_json::json!({"what": i.1, "why": i.2}))
        .collect();
    for item in extra {
        let what = item["what"].as_str().unwrap_or_default();
        let why = item["why"].as_str().unwrap_or_default();
        strings.push(format!("{what} ({why})"));
        details.push(item.clone());
    }
    (strings, details)
}

/// A first map for a global that has none (`propose`, US7): the depth that holds the most
/// values is the row node, keyed by `{ID}` (one level) or `{Key1}`…`{IdKey}`; one field per
/// piece (delimited) or element (`$LIST`) there, and per named sub-node one below. Returns the
/// map in model JSON and the path of every element it had to guess (names, delimiters, piece
/// counts), so a caller renames the fields to the class's properties before `check` or `replace`.
pub fn draft(report: &ShapeReport) -> (serde_json::Value, Vec<String>) {
    use serde_json::{json, Value};
    let at = "/maps/Master";
    let mut guesses = vec![format!("{at}/subscripts")];
    // The row depth: most value nodes, ignoring the root (often an ID counter).
    let levels = report
        .depths
        .iter()
        .filter_map(|(d, n)| Some((d.parse::<u32>().ok()?, *n)))
        .filter(|&(d, _)| d != 0)
        .max_by_key(|&(d, n)| (n, std::cmp::Reverse(d)))
        .map_or(1, |(d, _)| d);
    let subscripts: Vec<Value> = (1..=levels)
        .map(|l| {
            let expr = if l == levels {
                "{ID}".to_string()
            } else {
                format!("{{Key{l}}}")
            };
            json!({"level": l, "expression": expr})
        })
        .collect();
    let mut data = serde_json::Map::new();
    let mut add_fields = |prefix: &str, f: &Format| {
        let delimiter = match (f.format.as_str(), &f.delimiter) {
            ("$LIST", _) => None,
            (_, Some(d)) => Some(format!("\"{}\"", d.replace('"', "\"\""))),
            // A plain value: one field, read whole as the first piece of any delimiter.
            _ => Some("\"^\"".to_string()),
        };
        for i in 1..=f.pieces.max.max(1) {
            let name = format!("{prefix}{i}");
            let mut field = serde_json::Map::new();
            field.insert("name".into(), json!(name));
            if let Some(n) = &f.node {
                field.insert("node".into(), json!(n));
            }
            if let Some(d) = &delimiter {
                field.insert("delimiter".into(), json!(d));
            }
            field.insert("piece".into(), json!(i.to_string()));
            guesses.push(format!("{at}/data/{name}"));
            data.insert(name, Value::Object(field));
        }
    };
    let row = report
        .formats
        .iter()
        // A bit string is not a row of fields (the sampler names it `bits`): nothing is drafted from it.
        .filter(|f| f.depth == levels && f.node.is_none() && f.format != "bits")
        .max_by_key(|f| f.pieces.max);
    if let Some(row) = row {
        add_fields("Field", row);
    }
    for f in report
        .formats
        .iter()
        .filter(|f| f.depth == levels + 1 && f.format != "bits")
    {
        let Some(node) = &f.node else { continue };
        let mut prefix: String = node.chars().filter(|c| c.is_ascii_alphanumeric()).collect();
        if let Some(first) = prefix.get_mut(..1) {
            first.make_ascii_uppercase();
        }
        if prefix.is_empty() || prefix.starts_with(|c: char| c.is_ascii_digit()) {
            prefix = format!("Node{prefix}");
        }
        add_fields(&prefix, f);
    }
    let map = json!({
        "name": "Master",
        "type": "data",
        "global": report.global,
        "subscripts": subscripts,
        "data": data,
    });
    (map, guesses)
}
