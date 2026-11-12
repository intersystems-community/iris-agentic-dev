//! Validation of a Storage tree before any write (FR-014, data-model.md "Validation"). The rules
//! come from research R4: values IRIS rejects at compile time, or keeps and then misreads.

use super::path::{positive_integer, Level, KEYWORD_TAGS, MAP};
use super::tree::Node;

/// One broken rule: the path of the element, the rule's name, and the error code it answers
/// with (`STORAGE_MAP_INVALID`, or `STORAGE_MAP_READ_ONLY` for storage iad only reads).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Invalid {
    pub path: String,
    pub rule: String,
    pub code: &'static str,
}

const MAP_TYPES: &[&str] = &["data", "index", "bitmap", "bitmapextent", "columnar"];
const ACCESS_TYPES: &[&str] = &["sub", "piece", "list", "global", "other"];

/// Top-level elements besides the keywords, read from `%Dictionary.StorageDefinition` on 2026.2
/// (relationships by their element names). Top-level `Data` and `Property` are default storage
/// and statistics, whose insides are not checked. Inside a map, the levels are `path.rs`'s.
const TOP: &[&str] = &[
    "SQLMap",
    "Data",
    "Property",
    "Index",
    "ShardKey",
    "PartitionKey",
];

/// Every FR-014 rule `nodes` breaks, or `Ok`.
pub fn validate(nodes: &[Node]) -> Result<(), Vec<Invalid>> {
    let mut out = Vec::new();
    check(nodes, &mut out);
    ok_if_empty(out)
}

/// [`validate`] for a write from `before` to `after`: an element iad does not model that
/// `before` already had at the same path is allowed (IRIS dropped or kept it; the read-back
/// says which). Every other rule applies to all of `after`.
pub fn validate_change(before: &[Node], after: &[Node]) -> Result<(), Vec<Invalid>> {
    let mut had = Vec::new();
    check(before, &mut had);
    let had: Vec<String> = had
        .into_iter()
        .filter(|i| i.rule == "unknown_element")
        .map(|i| i.path)
        .collect();
    let mut found = Vec::new();
    check(after, &mut found);
    found.retain(|i| !(i.rule == "unknown_element" && had.contains(&i.path)));
    ok_if_empty(found)
}

fn ok_if_empty(found: Vec<Invalid>) -> Result<(), Vec<Invalid>> {
    if found.is_empty() {
        Ok(())
    } else {
        Err(found)
    }
}

fn invalid(out: &mut Vec<Invalid>, path: String, rule: &str) {
    let code = if rule == "read_only" {
        "STORAGE_MAP_READ_ONLY"
    } else {
        "STORAGE_MAP_INVALID"
    };
    out.push(Invalid {
        path,
        rule: rule.to_string(),
        code,
    });
}

fn check(nodes: &[Node], out: &mut Vec<Invalid>) {
    match nodes.iter().find(|n| n.tag == "Type" && n.is_leaf()) {
        Some(t) if t.value.as_deref().map(str::trim) == Some("%Storage.SQL") => {}
        _ => invalid(out, "/keywords/Type".into(), "read_only"),
    }
    for n in nodes {
        if n.is_leaf() && n.name.is_none() {
            let at = format!("/keywords/{}", n.tag);
            if !KEYWORD_TAGS.contains(&n.tag.as_str()) {
                invalid(out, format!("/keywords/extra/{}", n.tag), "unknown_element");
            } else {
                leaf_rules(n, &at, out);
            }
        } else if n.tag == "SQLMap" {
            let at = format!("/maps/{}", n.name.as_deref().unwrap_or(""));
            if n.name.as_deref().is_none_or(|s| s.trim().is_empty()) {
                invalid(out, at.clone(), "map_name");
            }
            element(n, &MAP, &at, out);
        } else if !TOP.contains(&n.tag.as_str()) {
            invalid(out, format!("/keywords/extra/{}", n.tag), "unknown_element");
        }
    }
}

/// The rules for the children of one modelled element at path `at`.
fn element(n: &Node, level: &Level, at: &str, out: &mut Vec<Invalid>) {
    for c in n.children.iter().flatten() {
        let Some((key, collection)) = level.child(&c.tag) else {
            invalid(out, format!("{at}/extra/{}", c.tag), "unknown_element");
            continue;
        };
        match (collection, &c.name) {
            (Some((inner, integer)), Some(name)) if !c.is_leaf() => {
                let here = format!("{at}/{key}/{name}");
                if integer && !positive_integer(name) {
                    invalid(out, here.clone(), "integer_name");
                }
                element(c, inner, &here, out);
            }
            _ => {
                let here = format!("{at}/{key}");
                leaf_rules(c, &here, out);
                let v = c.value.as_deref().unwrap_or("").trim();
                match (n.tag.as_str(), c.tag.as_str()) {
                    ("SQLMap", "Type") if v == "columnar" => invalid(out, here, "read_only"),
                    ("SQLMap", "Type") if !MAP_TYPES.contains(&v) => invalid(out, here, "map_type"),
                    ("Subscript", "AccessType")
                        if !ACCESS_TYPES.contains(&v.to_ascii_lowercase().as_str()) =>
                    {
                        invalid(out, here, "access_type")
                    }
                    _ => {}
                }
            }
        }
    }
}

/// Rules for any leaf: not empty, and no extended global reference in a tag that names a global.
fn leaf_rules(n: &Node, at: &str, out: &mut Vec<Invalid>) {
    let v = n.value.as_deref().unwrap_or("");
    if v.trim().is_empty() {
        invalid(out, at.to_string(), "empty_leaf");
        return;
    }
    let names_global = n.tag == "Global"
        || super::globals::GLOBAL_TAGS
            .iter()
            .any(|t| t.eq_ignore_ascii_case(&n.tag));
    let t = v.trim_start();
    if names_global && (t.starts_with("^|") || t.starts_with("^[")) {
        invalid(out, at.to_string(), "extended_reference");
    }
}
