//! Diagram generation for storage maps (spec 172 US2).
//!
//! Produces Mermaid flowcharts and JSON graph structures that visualize how a storage map
//! organizes data in globals.

use super::model::StorageModel;
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
            let text = format!(
                "sub {}: {} ({})",
                idx + 1,
                label(&sub.expression),
                label(&sub.access_type)
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
