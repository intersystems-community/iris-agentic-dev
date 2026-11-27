//! Diagram generation for storage maps (spec 172 US2).
//!
//! Produces Mermaid flowcharts and JSON graph structures that visualize how a storage map
//! organizes data in globals.

use super::model::{level_signs, StorageModel};
use serde::Serialize;

/// A node in the graph visualization.
#[derive(Debug, Clone, Serialize, PartialEq)]
pub struct GNode {
    pub id: String,
    pub kind: String,
    pub label: String,
}

/// An edge connecting two nodes.
#[derive(Debug, Clone, Serialize, PartialEq)]
pub struct GEdge {
    pub from: String,
    pub to: String,
}

/// The graph structure for JSON output.
#[derive(Debug, Clone, Serialize, PartialEq)]
pub struct Graph {
    pub nodes: Vec<GNode>,
    pub edges: Vec<GEdge>,
}

/// Escapes a string for safe use in Mermaid labels (FR-005).
///
/// Keeps: letters, digits, space, and `_.,:^(){}%$-`
/// Everything else becomes a Mermaid entity code `#<decimal>;`
fn label(s: &str) -> String {
    const KEPT: &str = " _.,:^(){}%$-";
    let mut result = String::with_capacity(s.len() * 2);
    let mut chars = s.chars().peekable();
    while let Some(ch) = chars.next() {
        // Mermaid finds `%%` directives anywhere in the text, quoted or not, so a `%` that
        // starts a pair is escaped and no label ever holds `%%`.
        let starts_pair = ch == '%' && chars.peek() == Some(&'%');
        if !starts_pair && (ch.is_ascii_alphanumeric() || KEPT.contains(ch)) {
            result.push(ch);
        } else {
            result.push_str(&format!("#{};", ch as u32));
        }
    }
    result
}

/// Generates a Mermaid flowchart diagram for the storage model.
///
/// If `map` is `Some`, diagrams only that map; otherwise diagrams all maps.
pub fn mermaid(m: &StorageModel, map: Option<&str>) -> String {
    let g = graph(m, map);
    let mut out = String::from("flowchart LR\n");
    for node in &g.nodes {
        out.push_str(&format!("  {}[\"{}\"]\n", node.id, node.label));
    }
    for edge in &g.edges {
        out.push_str(&format!("  {} --> {}\n", edge.from, edge.to));
    }
    out
}

/// Generates a JSON graph structure for the storage model.
///
/// If `map` is `Some`, graphs only that map; otherwise graphs all maps.
pub fn graph(m: &StorageModel, map_filter: Option<&str>) -> Graph {
    let mut nodes = Vec::new();
    let mut edges = Vec::new();
    let mut next_id = 1;

    let maps = m
        .storage
        .maps
        .iter()
        .filter(|map| map_filter.is_none_or(|name| map.name == name));

    for map in maps {
        // Each map is one chain: global, then each subscript, then data, then row ID.
        // Entries are (id prefix, kind, label).
        let mut chain = vec![("G", "global", label(&map.global))];

        for (idx, sub) in map.subscripts.iter().enumerate() {
            // A level the map computes by code (or filters by code) is said so in its own node: a
            // reader of the picture would otherwise count it as a subscript of the global.
            let signs = level_signs(sub);
            let by_code = if !signs.computed.is_empty() {
                ", computed by code, may not be a subscript"
            } else if signs.filter_only {
                ", keys filtered by code"
            } else {
                ""
            };
            let text = format!(
                "sub {}: {} ({}{})",
                idx + 1,
                label(&sub.expression),
                label(&sub.access_type),
                label(by_code)
            );
            chain.push(("S", "subscript", text));
        }

        if !map.data.is_empty() {
            let fields: Vec<String> = map
                .data
                .iter()
                .map(|(field_name, field)| match (&field.piece, &field.node) {
                    (Some(piece), _) => format!("{}: piece {}", label(field_name), label(piece)),
                    (None, Some(_)) => format!("{}: node", label(field_name)),
                    (None, None) => label(field_name),
                })
                .collect();
            chain.push(("D", "data", fields.join(", ")));
        }

        if !map.row_id.is_empty() {
            let parts: Vec<String> = map
                .row_id
                .iter()
                .filter_map(|part| part.field.as_deref().or(part.expression.as_deref()))
                .map(label)
                .collect();
            chain.push(("R", "row_id", format!("row ID: {}", parts.join(", "))));
        }

        let mut previous: Option<String> = None;
        for (prefix, kind, text) in chain {
            let id = format!("{prefix}{next_id}");
            next_id += 1;
            if let Some(from) = previous.replace(id.clone()) {
                edges.push(GEdge {
                    from,
                    to: id.clone(),
                });
            }
            nodes.push(GNode {
                id,
                kind: kind.to_string(),
                label: text,
            });
        }
    }

    Graph { nodes, edges }
}

// ───────────────────────────────────────────────────────────────────────────
// Markdown: the storage as a document a person reads
// ───────────────────────────────────────────────────────────────────────────

/// A value as a Markdown code span that is safe inside a table cell: `|` is escaped, and a value
/// holding a backtick gets a longer fence.
fn code(v: &str) -> String {
    let v = v.replace('|', "\\|");
    if v.contains('`') {
        format!("`` {v} ``")
    } else {
        format!("`{v}`")
    }
}

/// A table cell for a value that may be absent or span lines. A multi-line value is shown as a
/// code block after the table, and the cell says where.
fn cell(v: Option<&str>, label: String, blocks: &mut Vec<(String, String)>) -> String {
    match v {
        None => "—".to_string(),
        Some(v) if v.contains('\n') => {
            blocks.push((label.clone(), v.to_string()));
            format!("{label} (below)")
        }
        Some(v) => code(v),
    }
}

fn push_bullets(out: &mut String, items: &[&str]) {
    for item in items {
        out.push_str(&format!("- {item}\n"));
    }
    out.push('\n');
}

fn push_blocks(out: &mut String, blocks: Vec<(String, String)>) {
    for (label, body) in blocks {
        out.push_str(&format!("{label}:\n\n```objectscript\n{body}\n```\n\n"));
    }
}

/// The class's storage as one Markdown document: the storage keywords, then for each map (or
/// only `map`) its global layout, subscripts, fields, row ID, the facts `read` derives and its
/// Mermaid flowchart. Default storage is listed as its node slots. Built from the same model as
/// `read`, so it says nothing `read` does not.
pub fn markdown(m: &StorageModel, map: Option<&str>) -> String {
    let s = &m.storage;
    let mut out = format!("# Storage of {}\n\n", m.class);
    let state = if s.editable {
        "editable with iris_storage_map".to_string()
    } else {
        format!(
            "read-only here ({})",
            s.not_editable_reason
                .as_deref()
                .unwrap_or("not %Storage.SQL")
        )
    };
    out.push_str(&format!(
        "Storage `{}`, type `{}`, {state}.\n\n",
        s.name, s.r#type
    ));
    let shown: Vec<_> = s
        .keywords
        .iter()
        .filter(|(k, _)| !m.compiler_added.contains(k))
        .collect();
    if !shown.is_empty() {
        out.push_str("| Keyword | Value |\n| --- | --- |\n");
        for (k, v) in shown {
            out.push_str(&format!("| `{k}` | {} |\n", code(v)));
        }
        out.push('\n');
    }
    let storage_facts: Vec<&str> = m
        .derived
        .iter()
        .filter(|d| !d.path.starts_with("/maps/"))
        .map(|d| d.fact.as_str())
        .collect();
    if !storage_facts.is_empty() {
        push_bullets(&mut out, &storage_facts);
    }

    if let Some(ds) = &s.default_storage {
        out.push_str("## Default storage\n\n");
        out.push_str("| Location | Global |\n| --- | --- |\n");
        let stream_hidden = m.compiler_added.iter().any(|k| k == "StreamLocation");
        for (what, g) in [
            ("Data", &ds.data_location),
            ("ID", &ds.id_location),
            ("Index", &ds.index_location),
            ("Stream", &ds.stream_location),
        ] {
            if !g.is_empty() && !(what == "Stream" && stream_hidden) {
                out.push_str(&format!("| {what} | {} |\n", code(g)));
            }
        }
        out.push('\n');
        for node in &ds.nodes {
            out.push_str(&format!(
                "### Node `{}`\n\n| Slot | Property |\n| --- | --- |\n",
                node.node
            ));
            for slot in &node.slots {
                out.push_str(&format!("| {} | {} |\n", slot.slot, code(&slot.property)));
            }
            out.push('\n');
        }
    }

    for mp in s
        .maps
        .iter()
        .filter(|mp| map.is_none_or(|want| mp.name == want))
    {
        let mut role = vec![if mp.r#type.is_empty() {
            "no type".to_string()
        } else {
            mp.r#type.clone()
        }];
        if mp.master {
            role.push("master map".into());
        } else if mp.r#type == "data" {
            role.push("not used for reading or filing".into());
        }
        out.push_str(&format!("## Map `{}` ({})\n\n", mp.name, role.join(", ")));

        let mut subs: Vec<&super::model::Subscript> = mp.subscripts.iter().collect();
        subs.sort_by_key(|s| s.level);
        let layout = if subs.is_empty() {
            mp.global.clone()
        } else {
            let exprs: Vec<&str> = subs.iter().map(|s| s.expression.as_str()).collect();
            format!("{}({})", mp.global, exprs.join(","))
        };
        out.push_str(&format!("Global layout: {}\n\n", code(&layout)));

        let mut blocks = Vec::new();
        if !subs.is_empty() {
            out.push_str(
                "| Level | Expression | Access type | Delimiter | Other |\n| --- | --- | --- | --- | --- |\n",
            );
            for sb in &subs {
                let lvl = sb.level;
                let mut other = Vec::new();
                for (key, v) in [
                    ("data_access", &sb.data_access),
                    ("next_code", &sb.next_code),
                    ("start_value", &sb.start_value),
                    ("stop_value", &sb.stop_value),
                    ("stop_expression", &sb.stop_expression),
                    ("loop_init_value", &sb.loop_init_value),
                    ("null_marker", &sb.null_marker),
                ] {
                    if let Some(v) = v {
                        let shown = cell(Some(v), format!("Level {lvl} {key}"), &mut blocks);
                        // A multi-line value's cell already names it.
                        other.push(if v.contains('\n') {
                            shown
                        } else {
                            format!("{key} {shown}")
                        });
                    }
                }
                for av in &sb.access_vars {
                    other.push(format!(
                        "access var {} {} = {}",
                        av.level,
                        code(&av.variable),
                        cell(
                            Some(&av.code),
                            format!("Level {lvl} access var {}", av.level),
                            &mut blocks
                        )
                    ));
                }
                for ic in &sb.invalid_conditions {
                    other.push(format!(
                        "invalid if {}",
                        cell(
                            Some(&ic.expression),
                            format!("Level {lvl} invalid condition {}", ic.level),
                            &mut blocks
                        )
                    ));
                }
                out.push_str(&format!(
                    "| {lvl} | {} | {} | {} | {} |\n",
                    cell(
                        Some(&sb.expression),
                        format!("Level {lvl} expression"),
                        &mut blocks
                    ),
                    sb.access_type,
                    cell(
                        sb.delimiter.as_deref(),
                        format!("Level {lvl} delimiter"),
                        &mut blocks
                    ),
                    if other.is_empty() {
                        "—".to_string()
                    } else {
                        other.join("; ")
                    },
                ));
            }
            out.push('\n');
        }

        if !mp.data.is_empty() {
            out.push_str(
                "| Field | Node | Delimiter | Piece | Format | Filed |\n| --- | --- | --- | --- | --- | --- |\n",
            );
            for (name, d) in &mp.data {
                out.push_str(&format!(
                    "| {} | {} | {} | {} | {} | {} |\n",
                    code(name),
                    d.node.as_deref().map_or("row node".to_string(), code),
                    d.delimiter.as_deref().map_or("—".to_string(), code),
                    d.piece.as_deref().map_or("—".to_string(), code),
                    d.derived.format,
                    if d.derived.filed {
                        "yes"
                    } else {
                        "no (computed)"
                    },
                ));
            }
            out.push('\n');
            let computed: Vec<(&String, &str)> = mp
                .data
                .iter()
                .filter_map(|(n, d)| d.retrieval_code.as_deref().map(|c| (n, c)))
                .collect();
            if !computed.is_empty() {
                out.push_str("| Computed field | Retrieval code |\n| --- | --- |\n");
                for (n, c) in computed {
                    out.push_str(&format!(
                        "| {} | {} |\n",
                        code(n),
                        cell(Some(c), format!("Field {n} retrieval code"), &mut blocks)
                    ));
                }
                out.push('\n');
            }
        }

        if !mp.row_id.is_empty() {
            out.push_str("| Row ID part | Field | Expression |\n| --- | --- | --- |\n");
            for part in &mp.row_id {
                out.push_str(&format!(
                    "| {} | {} | {} |\n",
                    part.position,
                    part.field.as_deref().map_or("—".to_string(), code),
                    cell(
                        part.expression.as_deref(),
                        format!("Row ID part {} expression", part.position),
                        &mut blocks
                    ),
                ));
            }
            out.push('\n');
        }

        let mut options = Vec::new();
        for (key, v) in [
            ("condition", &mp.condition),
            ("condition_fields", &mp.condition_fields),
            ("conditional_with_host_vars", &mp.conditional_with_host_vars),
            ("row_reference", &mp.row_reference),
            ("structure", &mp.structure),
            ("population_type", &mp.population_type),
            ("population_pct", &mp.population_pct),
            ("block_count", &mp.block_count),
            ("deferred", &mp.deferred),
        ] {
            if let Some(v) = v {
                options.push(format!(
                    "- {key}: {}",
                    cell(Some(v), format!("Map {key}"), &mut blocks)
                ));
            }
        }
        for e in &mp.extra {
            options.push(format!("- not modelled by iad: `<{}>`", e.tag));
        }
        if !options.is_empty() {
            out.push_str(&options.join("\n"));
            out.push_str("\n\n");
        }
        push_blocks(&mut out, blocks);

        let prefix = format!("/maps/{}", mp.name);
        let inside = format!("{prefix}/");
        let facts: Vec<&str> = m
            .derived
            .iter()
            .filter(|d| d.path == prefix || d.path.starts_with(&inside))
            .map(|d| d.fact.as_str())
            .collect();
        if !facts.is_empty() {
            out.push_str("Facts:\n\n");
            push_bullets(&mut out, &facts);
        }
        out.push_str(&format!(
            "```mermaid\n{}```\n\n",
            mermaid(m, Some(&mp.name))
        ));
    }
    out.trim_end().to_string() + "\n"
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn label_keeps_safe_chars() {
        assert_eq!(label("abc123"), "abc123");
        assert_eq!(label("_.,:^(){}%$-"), "_.,:^(){}%$-");
        assert_eq!(label("Test Name"), "Test Name");
    }

    #[test]
    fn label_escapes_dangerous_chars() {
        // `click` stays as text: inside a quoted label it is inert once the quotes cannot break.
        let l = label("x\"];click A");
        assert_eq!(l, "x#34;#93;#59;click A");

        assert_eq!(label("%%{init"), "#37;%{init");
        assert_eq!(label("%%%"), "#37;#37;%");
        assert_eq!(label("%Library.String"), "%Library.String");
        for s in ["%%{init: {}}%%", "a%%%%b", "%%"] {
            assert!(!label(s).contains("%%"), "{s}");
        }
    }

    #[test]
    fn label_escapes_brackets_and_quotes() {
        let input = "test[]\"'";
        let output = label(input);
        assert!(!output.contains('['));
        assert!(!output.contains(']'));
        assert!(!output.contains('"'));
        assert!(!output.contains('\''));
    }
}
