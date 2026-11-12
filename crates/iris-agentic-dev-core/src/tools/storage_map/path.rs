//! Paths into a Storage tree and `edit`'s operations on it (data-model.md "Paths", "Path
//! operations"). A path names one element by the model's JSON keys; the operations act on the
//! element tree, so an element iad does not model (`extra`) is carried through untouched.

use super::tree::Node;
use serde_json::Value;

/// One step of a path: an element named by tag and `name`, a leaf by tag, or an element iad
/// does not model (`extra`), by tag.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PathSeg {
    Elem { tag: String, name: String },
    Leaf(String),
    Extra(String),
}

/// What may sit under one kind of element: named collections (JSON key, element tag, whether the
/// names are integers) and leaves (JSON key, tag).
pub(super) struct Level {
    collections: &'static [(&'static str, &'static str, bool)],
    leaves: &'static [(&'static str, &'static str)],
}

impl Level {
    /// The JSON key of a child tagged `tag`, and for a named collection the level of its elements
    /// and whether their names are integers. `None` when this level does not model `tag`.
    pub(super) fn child(
        &self,
        tag: &str,
    ) -> Option<(&'static str, Option<(&'static Level, bool)>)> {
        if let Some((key, _)) = self.leaves.iter().find(|(_, t)| *t == tag) {
            return Some((key, None));
        }
        self.collections
            .iter()
            .find(|(_, t, _)| *t == tag)
            .map(|(key, t, integer)| (*key, Some((level_of(t), *integer))))
    }
}

/// The children of an `SQLMap`.
pub(super) const MAP: Level = Level {
    collections: &[
        ("subscripts", "Subscript", true),
        ("data", "Data", false),
        ("row_id", "RowIdSpec", true),
    ],
    leaves: &[
        ("global", "Global"),
        ("type", "Type"),
        ("condition", "Condition"),
        ("condition_fields", "ConditionFields"),
        ("conditional_with_host_vars", "ConditionalWithHostVars"),
        ("population_type", "PopulationType"),
        ("population_pct", "PopulationPct"),
        ("block_count", "BlockCount"),
        ("structure", "Structure"),
        ("row_reference", "RowReference"),
        ("deferred", "Deferred"),
    ],
};

const SUBSCRIPT: Level = Level {
    collections: &[
        ("access_vars", "Accessvar", true),
        ("invalid_conditions", "Invalidcondition", true),
    ],
    leaves: &[
        ("expression", "Expression"),
        ("access_type", "AccessType"),
        ("delimiter", "Delimiter"),
        ("data_access", "DataAccess"),
        ("next_code", "NextCode"),
        ("start_value", "StartValue"),
        ("stop_value", "StopValue"),
        ("stop_expression", "StopExpression"),
        ("loop_init_value", "LoopInitValue"),
        ("null_marker", "NullMarker"),
    ],
};

const DATA: Level = Level {
    collections: &[],
    leaves: &[
        ("node", "Node"),
        ("delimiter", "Delimiter"),
        ("piece", "Piece"),
        ("retrieval_code", "RetrievalCode"),
    ],
};

const ROW_ID: Level = Level {
    collections: &[],
    leaves: &[("field", "Field"), ("expression", "Expression")],
};

const ACCESS_VAR: Level = Level {
    collections: &[],
    leaves: &[("code", "Code"), ("variable", "Variable")],
};

const INVALID_CONDITION: Level = Level {
    collections: &[],
    leaves: &[("expression", "Expression")],
};

/// The storage-level leaves (`%Dictionary.StorageDefinition`'s plain properties, read from the
/// compiled dictionary on 2026.2), addressed as `/keywords/<Tag>`.
pub const KEYWORD_TAGS: &[&str] = &[
    "CounterLocation",
    "DataLocation",
    "DefaultData",
    "Deprecated",
    "Description",
    "ExtentLocation",
    "ExtentSize",
    "Final",
    "IdExpression",
    "IdFunction",
    "IdLocation",
    "IndexLocation",
    "Internal",
    "Partitioned",
    "SequenceNumber",
    "ShardLocalClassName",
    "ShardLocalTableName",
    "Sharded",
    "SqlChildSub",
    "SqlIdExpression",
    "SqlRowIdName",
    "SqlRowIdProperty",
    "SqlTableNumber",
    "State",
    "StreamLocation",
    "Type",
    "VersionLocation",
];

fn level_of(tag: &str) -> &'static Level {
    match tag {
        "SQLMap" => &MAP,
        "Subscript" => &SUBSCRIPT,
        "Data" => &DATA,
        "RowIdSpec" => &ROW_ID,
        "Accessvar" => &ACCESS_VAR,
        _ => &INVALID_CONDITION,
    }
}

/// `1`, `2`, `10`: a positive integer written the way IRIS writes it.
pub(super) fn positive_integer(s: &str) -> bool {
    !s.is_empty() && !s.starts_with('0') && s.bytes().all(|b| b.is_ascii_digit())
}

/// The steps of `p`, or why it names nothing.
pub fn parse_path(p: &str) -> Result<Vec<PathSeg>, String> {
    let rest = p
        .strip_prefix('/')
        .ok_or_else(|| format!("path {p:?} must start with /"))?;
    let segs: Vec<&str> = rest.split('/').collect();
    if segs.iter().any(|s| s.is_empty()) {
        return Err(format!("path {p:?} has an empty step"));
    }
    match segs.as_slice() {
        ["keywords", tag] if KEYWORD_TAGS.contains(tag) => Ok(vec![PathSeg::Leaf(tag.to_string())]),
        ["keywords", tag] => Err(format!("{p}: {tag} is not a storage keyword")),
        ["maps", name, more @ ..] => {
            let mut out = vec![PathSeg::Elem {
                tag: "SQLMap".into(),
                name: name.to_string(),
            }];
            let mut level = &MAP;
            let mut i = 0;
            while i < more.len() {
                let key = more[i];
                let last = i + 1 == more.len();
                if key == "extra" {
                    match &more[i + 1..] {
                        [tag] if level.child(tag).is_none() => {
                            out.push(PathSeg::Extra(tag.to_string()));
                            return Ok(out);
                        }
                        [tag] => {
                            return Err(format!("{p}: {tag} is a modelled element, not extra"))
                        }
                        _ => return Err(format!("{p}: extra takes one element tag")),
                    }
                }
                if let Some((_, tag, integer)) =
                    level.collections.iter().find(|(k, _, _)| *k == key)
                {
                    let Some(name) = more.get(i + 1) else {
                        return Err(format!("{p}: {key} needs a name"));
                    };
                    if *integer && !positive_integer(name) {
                        return Err(format!("{p}: {key} are numbered 1, 2, …; {name:?} is not"));
                    }
                    out.push(PathSeg::Elem {
                        tag: tag.to_string(),
                        name: name.to_string(),
                    });
                    level = level_of(tag);
                    i += 2;
                    continue;
                }
                if let Some((_, tag)) = level.leaves.iter().find(|(k, _)| *k == key) {
                    if !last {
                        return Err(format!("{p}: {key} is a leaf; nothing goes under it"));
                    }
                    out.push(PathSeg::Leaf(tag.to_string()));
                    return Ok(out);
                }
                return Err(format!("{p}: no {key:?} here"));
            }
            Ok(out)
        }
        _ => Err(format!("path {p:?} must start with /maps/ or /keywords/")),
    }
}

/// The path of every leaf a path can name, in tree order.
pub fn leaf_paths(nodes: &[Node]) -> Vec<String> {
    collect(nodes, false)
}

/// The path of every element and leaf a path can name, in tree order: [`leaf_paths`] plus the
/// maps, subscripts, fields, row ID parts, access variables and invalid conditions themselves.
pub fn paths(nodes: &[Node]) -> Vec<String> {
    collect(nodes, true)
}

fn collect(nodes: &[Node], elements: bool) -> Vec<String> {
    let mut out = Vec::new();
    for n in nodes {
        if n.is_leaf() && n.name.is_none() && KEYWORD_TAGS.contains(&n.tag.as_str()) {
            out.push(format!("/keywords/{}", n.tag));
        } else if n.tag == "SQLMap" {
            if let Some(name) = &n.name {
                let at = format!("/maps/{name}");
                if elements {
                    out.push(at.clone());
                }
                walk_leaves(n, &MAP, &at, elements, &mut out);
            }
        }
    }
    out
}

fn walk_leaves(n: &Node, level: &Level, at: &str, elements: bool, out: &mut Vec<String>) {
    for c in n.children.iter().flatten() {
        let leaf = c.is_leaf() && c.name.is_none();
        match (level.child(&c.tag), &c.name) {
            (Some((key, None)), _) if leaf => out.push(format!("{at}/{key}")),
            (Some((key, Some((inner, _)))), Some(name)) => {
                let here = format!("{at}/{key}/{name}");
                if elements {
                    out.push(here.clone());
                }
                walk_leaves(c, inner, &here, elements, out);
            }
            _ => {}
        }
    }
}

fn matches(n: &Node, seg: &PathSeg) -> bool {
    match seg {
        PathSeg::Elem { tag, name } => n.tag == *tag && n.name.as_deref() == Some(name.as_str()),
        PathSeg::Leaf(tag) => n.tag == *tag && n.name.is_none() && n.is_leaf(),
        PathSeg::Extra(tag) => n.tag == *tag,
    }
}

/// The node `path` names, if it is there.
pub fn get<'a>(nodes: &'a [Node], path: &str) -> Option<&'a Node> {
    let segs = parse_path(path).ok()?;
    let mut here = nodes;
    let (last, parents) = segs.split_last()?;
    for seg in parents {
        here = here.iter().find(|n| matches(n, seg))?.children.as_deref()?;
    }
    here.iter().find(|n| matches(n, last))
}

/// The children of the element `parents` names, to change.
fn children_mut<'a>(
    nodes: &'a mut Vec<Node>,
    parents: &[PathSeg],
    path: &str,
) -> Result<&'a mut Vec<Node>, String> {
    let mut here = nodes;
    for seg in parents {
        let n = here
            .iter_mut()
            .find(|n| matches(n, seg))
            .ok_or_else(|| format!("{path}: {seg:?} is not there"))?;
        here = n
            .children
            .as_mut()
            .ok_or_else(|| format!("{path}: {seg:?} is a leaf"))?;
    }
    Ok(here)
}

/// Apply `ops` in order to a copy of `nodes`; on success the copy replaces `nodes` and the
/// answer is every path an op changed, once each. If any op fails, `nodes` is unchanged.
pub fn apply(nodes: &mut Vec<Node>, ops: &[Value]) -> Result<Vec<String>, String> {
    let mut work = nodes.clone();
    let mut changed: Vec<String> = Vec::new();
    for (i, op) in ops.iter().enumerate() {
        for p in apply_one(&mut work, op).map_err(|e| format!("op {i}: {e}"))? {
            if !changed.contains(&p) {
                changed.push(p);
            }
        }
    }
    *nodes = work;
    Ok(changed)
}

fn apply_one(work: &mut Vec<Node>, op: &Value) -> Result<Vec<String>, String> {
    let kind = op
        .get("op")
        .and_then(Value::as_str)
        .ok_or("an op needs `op`")?;
    let path = op
        .get("path")
        .and_then(Value::as_str)
        .ok_or("an op needs `path`")?;
    let segs = parse_path(path)?;
    let (last, parents) = segs.split_last().ok_or("empty path")?;
    let value = op.get("value").unwrap_or(&Value::Null);
    let siblings = children_mut(work, parents, path)?;
    let at = siblings.iter().position(|n| matches(n, last));
    match (kind, last, value) {
        ("set", PathSeg::Extra(_), _) | ("add", PathSeg::Extra(_), _) => Err(format!(
            "{path}: an element iad does not model can only be removed"
        )),
        ("set", _, Value::Null) | ("remove", _, _) => {
            let i = at.ok_or_else(|| format!("{path}: nothing there to remove"))?;
            siblings.remove(i);
            Ok(vec![path.to_string()])
        }
        ("set", PathSeg::Leaf(tag), Value::String(s)) => {
            match at {
                Some(i) => siblings[i].value = Some(s.clone()),
                None => siblings.push(Node {
                    tag: tag.clone(),
                    name: None,
                    value: Some(s.clone()),
                    children: None,
                }),
            }
            Ok(vec![path.to_string()])
        }
        ("set", PathSeg::Leaf(_), _) => Err(format!("{path}: a leaf takes text or null")),
        ("set", PathSeg::Elem { tag, name }, v @ Value::Object(_)) => {
            let i = at.ok_or_else(|| format!("{path}: nothing there to set; use add"))?;
            siblings[i] = super::model::element_node(tag, name, v)?;
            Ok(vec![path.to_string()])
        }
        ("set", PathSeg::Elem { .. }, _) => {
            Err(format!("{path}: an element takes an object or null"))
        }
        ("add", PathSeg::Elem { tag, name }, v @ Value::Object(_)) => {
            if at.is_some() {
                return Err(format!("{path}: already there; use set"));
            }
            siblings.push(super::model::element_node(tag, name, v)?);
            Ok(vec![path.to_string()])
        }
        ("add", PathSeg::Elem { .. }, _) => Err(format!("{path}: add takes an object")),
        ("add", PathSeg::Leaf(_), _) => Err(format!("{path}: a leaf is set, not added")),
        ("move", PathSeg::Elem { tag, name }, _) => {
            let to = op
                .get("to")
                .and_then(Value::as_str)
                .ok_or("move needs `to`")?;
            let to_segs = parse_path(to)?;
            let (to_last, to_parents) = to_segs.split_last().ok_or("empty `to`")?;
            let PathSeg::Elem {
                tag: to_tag,
                name: to_name,
            } = to_last
            else {
                return Err(format!("{to}: move goes to an element"));
            };
            if to_parents != parents || to_tag != tag {
                return Err(format!("{path} -> {to}: move renumbers within one element"));
            }
            if !positive_integer(name) || !positive_integer(to_name) {
                return Err(format!("{path}: only numbered elements move"));
            }
            let i = at.ok_or_else(|| format!("{path}: nothing there to move"))?;
            if siblings.iter().any(|n| matches(n, to_last)) {
                return Err(format!("{to}: already there"));
            }
            siblings[i].name = Some(to_name.clone());
            Ok(vec![path.to_string(), to.to_string()])
        }
        ("move", _, _) => Err(format!("{path}: only numbered elements move")),
        (other, _, _) => Err(format!(
            "unknown op {other:?}; use set, add, remove or move"
        )),
    }
}
