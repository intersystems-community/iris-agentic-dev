//! T046: Mermaid diagram generation for storage maps (spec 172 US2).
//!
//! Run: `RUST_TEST_THREADS=12 cargo test --features testing --test unit test_storage_map_diagram_172`

use iris_agentic_dev_core::tools::storage_map::diagram::{graph, mermaid};
use iris_agentic_dev_core::tools::storage_map::model::{
    DataDerived, DataField, ExtraElement, RowIdPart, SqlMap, SqlStorage, StorageModel, Subscript,
};
use std::collections::BTreeMap;

fn build_member_model() -> StorageModel {
    let mut data = BTreeMap::new();
    data.insert(
        "Name".to_string(),
        DataField {
            name: "Name".to_string(),
            node: Some("1".to_string()),
            delimiter: Some("^".to_string()),
            piece: Some("1".to_string()),
            retrieval_code: None,
            extra: vec![],
            derived: DataDerived {
                format: "delimited".to_string(),
                computed: false,
                filed: true,
            },
        },
    );
    data.insert(
        "City".to_string(),
        DataField {
            name: "City".to_string(),
            node: Some("1".to_string()),
            delimiter: Some("^".to_string()),
            piece: Some("2".to_string()),
            retrieval_code: None,
            extra: vec![],
            derived: DataDerived {
                format: "delimited".to_string(),
                computed: false,
                filed: true,
            },
        },
    );

    let map = SqlMap {
        name: "Master".to_string(),
        version: "abc123".to_string(),
        r#type: "data".to_string(),
        master: true,
        used: true,
        global: "^IadSm172Member".to_string(),
        subscripts: vec![Subscript {
            level: 1,
            expression: "{ID}".to_string(),
            access_type: "Sub".to_string(),
            delimiter: None,
            data_access: None,
            next_code: None,
            start_value: None,
            stop_value: None,
            stop_expression: None,
            loop_init_value: None,
            null_marker: None,
            access_vars: vec![],
            invalid_conditions: vec![],
            extra: vec![],
        }],
        data,
        row_id: vec![RowIdPart {
            position: 1,
            field: Some("ID".to_string()),
            expression: None,
        }],
        condition: None,
        condition_fields: None,
        conditional_with_host_vars: None,
        population_type: None,
        population_pct: None,
        block_count: None,
        structure: None,
        row_reference: None,
        deferred: None,
        extra: vec![],
    };

    StorageModel {
        model_version: 1,
        class: "IadSm172.Member".to_string(),
        storage: SqlStorage {
            name: "SQLStorage".to_string(),
            r#type: "%Storage.SQL".to_string(),
            version: "def456".to_string(),
            editable: true,
            not_editable_reason: None,
            keywords: BTreeMap::new(),
            description: None,
            maps: vec![map],
            default_storage: None,
            extra: vec![],
        },
        derived: vec![],
    }
}

#[test]
fn member_gives_expected_node_list() {
    let m = build_member_model();
    let g = graph(&m, None);

    // Should have: global node, subscript node, data node, row_id node
    assert!(
        g.nodes.len() >= 4,
        "Expected at least 4 nodes: {:?}",
        g.nodes
    );

    // Check global node exists
    let global_node = g.nodes.iter().find(|n| n.kind == "global");
    assert!(global_node.is_some(), "Expected global node: {:?}", g.nodes);
    assert_eq!(global_node.unwrap().label, "^IadSm172Member");

    // Check subscript node
    let sub_node = g.nodes.iter().find(|n| n.kind == "subscript");
    assert!(sub_node.is_some(), "Expected subscript node: {:?}", g.nodes);
    assert!(sub_node.unwrap().label.contains("{ID}"));

    // Check data node
    let data_node = g.nodes.iter().find(|n| n.kind == "data");
    assert!(data_node.is_some(), "Expected data node: {:?}", g.nodes);

    // Check row_id node
    let rowid_node = g.nodes.iter().find(|n| n.kind == "row_id");
    assert!(rowid_node.is_some(), "Expected row_id node: {:?}", g.nodes);

    // Check edges exist
    assert!(
        g.edges.len() >= 3,
        "Expected at least 3 edges: {:?}",
        g.edges
    );
}

#[test]
fn injection_attempts_are_escaped() {
    let mut m = build_member_model();

    // Inject malicious field name with Mermaid syntax
    let malicious_field = DataField {
        name: "x\"];click A call alert()".to_string(),
        node: Some("2".to_string()),
        delimiter: None,
        piece: None,
        retrieval_code: None,
        extra: vec![],
        derived: DataDerived {
            format: "whole node".to_string(),
            computed: false,
            filed: true,
        },
    };
    m.storage.maps[0]
        .data
        .insert("BadField".to_string(), malicious_field);

    // Add malicious value in extra element
    m.storage.maps[0].extra.push(ExtraElement {
        tag: "Test".to_string(),
        name: Some("%%{init".to_string()),
        value: Some("dangerous".to_string()),
    });

    let output = mermaid(&m, None);

    // Must not contain injection strings
    assert!(
        !output.contains("click"),
        "Output contains 'click': {}",
        output
    );
    assert!(
        !output.contains("\"];"),
        "Output contains '\"];': {}",
        output
    );
    assert!(
        !output.contains("%%{"),
        "Output contains '%%{{': {}",
        output
    );
    assert!(
        !output.contains("alert()"),
        "Output contains 'alert()': {}",
        output
    );

    // Check graph too
    let g = graph(&m, None);
    for node in &g.nodes {
        assert!(
            !node.label.contains("click"),
            "Node label contains 'click': {}",
            node.label
        );
        assert!(
            !node.label.contains("\"];"),
            "Node label contains '\"];': {}",
            node.label
        );
        assert!(
            !node.label.contains("%%{"),
            "Node label contains '%%{{': {}",
            node.label
        );
    }
}

/// FR-005, on the text the diagram really draws: map name, global, subscript expression and the
/// data field keys. Each output line must be one the generator writes, and no label may hold a
/// raw `"` or `]` that would close it, whatever the map text holds.
#[test]
fn hostile_map_text_cannot_leave_its_label() {
    let mut m = build_member_model();
    let evil = "x\"]\nclick G1 call alert()\n%%{init: {}}%%\nG1[\"";
    m.storage.maps[0].name = evil.to_string();
    m.storage.maps[0].global = format!("^G{evil}");
    if let Some(s) = m.storage.maps[0].subscripts.first_mut() {
        s.expression = evil.to_string();
    }
    let field = m.storage.maps[0]
        .data
        .values()
        .next()
        .cloned()
        .expect("a field");
    m.storage.maps[0].data.insert(evil.to_string(), field);

    let out = mermaid(&m, None);
    assert!(!out.contains("%%"), "a Mermaid directive survived: {out}");
    let lines: Vec<&str> = out.lines().collect();
    assert!(lines.len() >= 3, "{out}");
    assert_eq!(lines[0].trim(), "flowchart LR", "{out}");
    for line in &lines[1..] {
        let t = line.trim();
        let node = t
            .split_once("[\"")
            .filter(|(id, rest)| {
                id.chars().all(|c| c.is_ascii_alphanumeric() || c == '_') && rest.ends_with("\"]")
            })
            .map(|(_, rest)| &rest[..rest.len() - 2]);
        let edge = t.contains(" --> ")
            && t.split(" --> ")
                .all(|p| p.chars().all(|c| c.is_ascii_alphanumeric() || c == '_'));
        let block = t.is_empty();
        match node {
            Some(inner) => assert!(
                !inner.contains('"') && !inner.contains(']') && !inner.contains('\n'),
                "label escapes its quotes: {line}"
            ),
            None => assert!(
                edge || block,
                "a line the generator never writes: {line}\n{out}"
            ),
        }
    }
}
