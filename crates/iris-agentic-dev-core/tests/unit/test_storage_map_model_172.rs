//! Tests for storage map typed model (T037–T042).

use iris_agentic_dev_core::tools::storage_map::model::*;
use iris_agentic_dev_core::tools::storage_map::tree::{parse_body, split_class, Node};
use std::fs;

/// Helper to load and parse a fixture class.
fn load_fixture(name: &str) -> (String, String, Vec<Node>) {
    let path = format!("tests/fixtures/storage172/classes/{}.cls", name);
    let text = fs::read_to_string(&path).expect("fixture not found");
    let spans = split_class(&text).expect("split failed");
    assert!(!spans.is_empty(), "no storage found");
    let span = &spans[0];
    let body = &text[span.body_start..span.body_end];
    let nodes = parse_body(body).expect("parse failed");
    (format!("IadSm172.{}", name), span.name.clone(), nodes)
}

// ───────────────────────────────────────────────────────────────────────────
// T037: Typed structs serialize to documented keys
// ───────────────────────────────────────────────────────────────────────────

#[test]
fn test_sql_map_serialization_t037() {
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
        data: vec![(
            "Name".to_string(),
            DataField {
                name: "Name".to_string(),
                node: None,
                delimiter: Some("\"^\"".to_string()),
                piece: Some("1".to_string()),
                retrieval_code: None,
                extra: vec![],
                derived: DataDerived {
                    format: "delimited".to_string(),
                    computed: false,
                    filed: true,
                },
            },
        )]
        .into_iter()
        .collect(),
        row_id: vec![],
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

    let val = serde_json::to_value(&map).unwrap();
    assert_eq!(val["name"], "Master");
    assert_eq!(val["version"], "abc123");
    assert_eq!(val["type"], "data");
    assert_eq!(val["master"], true);
    assert_eq!(val["used"], true);
    assert_eq!(val["global"], "^IadSm172Member");
    assert!(val["subscripts"].is_array());
    assert_eq!(val["subscripts"][0]["level"], 1);
    assert_eq!(val["subscripts"][0]["expression"], "{ID}");
    assert_eq!(val["subscripts"][0]["access_type"], "Sub");
    assert!(val["data"].is_object());
    assert_eq!(val["data"]["Name"]["name"], "Name");
    assert_eq!(val["data"]["Name"]["delimiter"], "\"^\"");
    assert_eq!(val["data"]["Name"]["piece"], "1");
    assert_eq!(val["data"]["Name"]["derived"]["format"], "delimited");
    assert_eq!(val["data"]["Name"]["derived"]["computed"], false);
    assert_eq!(val["data"]["Name"]["derived"]["filed"], true);
}

// ───────────────────────────────────────────────────────────────────────────
// T038: from_tree builds model from parsed nodes
// ───────────────────────────────────────────────────────────────────────────

#[test]
fn test_from_tree_member_t038() {
    let (class, storage_name, nodes) = load_fixture("Member");
    let model = from_tree(&class, &storage_name, &nodes);

    assert_eq!(model.model_version, MODEL_VERSION);
    assert_eq!(model.class, "IadSm172.Member");
    assert_eq!(model.storage.name, "SQLStorage");
    assert_eq!(model.storage.r#type, "%Storage.SQL");
    assert!(model.storage.editable);
    assert_eq!(model.storage.not_editable_reason, None);
    assert_eq!(
        model.storage.keywords.get("SqlIdExpression"),
        Some(&"$i(^IadSm172Member)".to_string())
    );
    assert_eq!(
        model.storage.keywords.get("StreamLocation"),
        Some(&"^IadSm172MemberS".to_string())
    );

    assert_eq!(model.storage.maps.len(), 1);
    let map = &model.storage.maps[0];
    assert_eq!(map.name, "Master");
    assert_eq!(map.r#type, "data");
    assert_eq!(map.global, "^IadSm172Member");

    assert_eq!(map.subscripts.len(), 1);
    assert_eq!(map.subscripts[0].level, 1);
    assert_eq!(map.subscripts[0].expression, "{ID}");

    assert_eq!(map.data.len(), 3);
    let name_field = map.data.get("Name").unwrap();
    assert_eq!(name_field.delimiter, Some("\"^\"".to_string()));
    assert_eq!(name_field.piece, Some("1".to_string()));
    assert_eq!(name_field.node, None);

    let city_field = map.data.get("City").unwrap();
    assert_eq!(city_field.piece, Some("2".to_string()));

    let age_field = map.data.get("Age").unwrap();
    assert_eq!(age_field.piece, Some("3".to_string()));
}

#[test]
fn test_from_tree_bookmulti_t038() {
    let (class, storage_name, nodes) = load_fixture("BookMulti");
    let model = from_tree(&class, &storage_name, &nodes);

    assert_eq!(model.storage.maps.len(), 1);
    let map = &model.storage.maps[0];
    assert_eq!(map.data.len(), 3);

    let title_field = map.data.get("Title").unwrap();
    assert_eq!(title_field.node, Some("\"title\"".to_string()));
    assert_eq!(title_field.delimiter, None);
    assert_eq!(title_field.piece, None);

    let pub_field = map.data.get("Publisher").unwrap();
    assert_eq!(pub_field.node, Some("\"pub\"".to_string()));

    let loc_field = map.data.get("Location").unwrap();
    assert_eq!(loc_field.node, Some("\"loc\"".to_string()));
}

#[test]
fn test_from_tree_copy_t038() {
    let (class, storage_name, nodes) = load_fixture("Copy");
    let model = from_tree(&class, &storage_name, &nodes);

    let map = &model.storage.maps[0];
    assert_eq!(map.subscripts.len(), 3); // BookId, Branch, CopyNo
    assert_eq!(map.subscripts[0].level, 1);
    assert_eq!(map.subscripts[1].level, 2);
    assert_eq!(map.subscripts[2].level, 3);
}

#[test]
fn test_from_tree_shelf_t038() {
    let (class, storage_name, nodes) = load_fixture("Shelf");
    let model = from_tree(&class, &storage_name, &nodes);

    let map = &model.storage.maps[0];
    // Shelf has 1 subscript with NextCode inside it
    assert_eq!(map.subscripts.len(), 1);
    assert!(map.subscripts[0].next_code.is_some());
}

#[test]
fn test_from_tree_non_integer_name_warning_t038() {
    // Build a tree with a non-integer subscript name
    let nodes = vec![
        Node {
            tag: "Type".to_string(),
            name: None,
            value: Some("%Storage.SQL".to_string()),
            children: None,
        },
        Node {
            tag: "SQLMap".to_string(),
            name: Some("Master".to_string()),
            value: None,
            children: Some(vec![
                Node {
                    tag: "Type".to_string(),
                    name: None,
                    value: Some("data".to_string()),
                    children: None,
                },
                Node {
                    tag: "Subscript".to_string(),
                    name: Some("abc".to_string()), // Not an integer
                    value: None,
                    children: Some(vec![Node {
                        tag: "Expression".to_string(),
                        name: None,
                        value: Some("{ID}".to_string()),
                        children: None,
                    }]),
                },
            ]),
        },
    ];

    let model = from_tree("Test.Class", "TestStorage", &nodes);
    // Should have a derived warning about non-integer subscript name
    let warning = model
        .derived
        .iter()
        .find(|d| d.fact.contains("non-integer"));
    assert!(
        warning.is_some(),
        "Expected derived warning for non-integer subscript name"
    );
}

// ───────────────────────────────────────────────────────────────────────────
// T039: Derived facts (R5 + FR-003)
// ───────────────────────────────────────────────────────────────────────────

#[test]
fn test_derived_facts_no_delimiter_t039() {
    let (class, storage_name, nodes) = load_fixture("BookMulti");
    let model = from_tree(&class, &storage_name, &nodes);
    let facts = derived_facts(&model);

    // BookMulti fields have no Delimiter, so they're read as $LIST
    let list_facts: Vec<_> = facts
        .iter()
        .filter(|d| d.fact.contains("no delimiter: node read as $LIST"))
        .collect();
    assert_eq!(list_facts.len(), 3); // Title, Publisher, Location
}

#[test]
fn test_derived_facts_master_map_t039() {
    let (class, storage_name, nodes) = load_fixture("LoanList");
    let model = from_tree(&class, &storage_name, &nodes);

    // First data map should be master
    assert!(model.storage.maps[0].master);
    if model.storage.maps.len() > 1 {
        assert!(!model.storage.maps[1].master);
    }
}

#[test]
fn test_derived_facts_later_maps_unused_t039() {
    // Build a model with two data maps
    let nodes = vec![
        Node {
            tag: "Type".to_string(),
            name: None,
            value: Some("%Storage.SQL".to_string()),
            children: None,
        },
        Node {
            tag: "SQLMap".to_string(),
            name: Some("Master".to_string()),
            value: None,
            children: Some(vec![
                Node {
                    tag: "Type".to_string(),
                    name: None,
                    value: Some("data".to_string()),
                    children: None,
                },
                Node {
                    tag: "Global".to_string(),
                    name: None,
                    value: Some("^Test".to_string()),
                    children: None,
                },
            ]),
        },
        Node {
            tag: "SQLMap".to_string(),
            name: Some("Other".to_string()),
            value: None,
            children: Some(vec![
                Node {
                    tag: "Type".to_string(),
                    name: None,
                    value: Some("data".to_string()),
                    children: None,
                },
                Node {
                    tag: "Global".to_string(),
                    name: None,
                    value: Some("^Other".to_string()),
                    children: None,
                },
            ]),
        },
    ];

    let model = from_tree("Test.Class", "TestStorage", &nodes);
    assert!(model.storage.maps[0].used);
    assert!(!model.storage.maps[1].used);
}

#[test]
fn test_derived_facts_retrieval_code_t039() {
    // Build a model with a RetrievalCode field
    let nodes = vec![
        Node {
            tag: "Type".to_string(),
            name: None,
            value: Some("%Storage.SQL".to_string()),
            children: None,
        },
        Node {
            tag: "SQLMap".to_string(),
            name: Some("Master".to_string()),
            value: None,
            children: Some(vec![
                Node {
                    tag: "Type".to_string(),
                    name: None,
                    value: Some("data".to_string()),
                    children: None,
                },
                Node {
                    tag: "Global".to_string(),
                    name: None,
                    value: Some("^Test".to_string()),
                    children: None,
                },
                Node {
                    tag: "Data".to_string(),
                    name: Some("ComputedField".to_string()),
                    value: None,
                    children: Some(vec![Node {
                        tag: "RetrievalCode".to_string(),
                        name: None,
                        value: Some("{ID}*2".to_string()),
                        children: None,
                    }]),
                },
            ]),
        },
    ];

    let model = from_tree("Test.Class", "TestStorage", &nodes);
    let facts = derived_facts(&model);

    // Should have a computed field fact
    let computed_facts: Vec<_> = facts
        .iter()
        .filter(|d| d.fact.contains("computed field"))
        .collect();
    assert_eq!(computed_facts.len(), 1);

    // The field with RetrievalCode should be marked as computed, not filed
    let map = &model.storage.maps[0];
    let computed_field = map.data.get("ComputedField").unwrap();
    assert!(computed_field.derived.computed);
    assert!(!computed_field.derived.filed);
}

#[test]
fn test_derived_facts_table_writable_t039() {
    let (class, storage_name, nodes) = load_fixture("Member");
    let model = from_tree(&class, &storage_name, &nodes);
    let facts = derived_facts(&model);

    // Member's global starts with ^, so table is writable
    let writable = facts.iter().any(|d| d.fact.contains("table is writable"));
    assert!(writable);
}

// ───────────────────────────────────────────────────────────────────────────
// T040: D11 facts and nested maps
// ───────────────────────────────────────────────────────────────────────────

#[test]
fn test_d11_row_node_with_fields_t040() {
    let (class, storage_name, nodes) = load_fixture("MemberD11");
    let facts = derived_facts(&from_tree(&class, &storage_name, &nodes));
    let d11 = facts
        .iter()
        .find(|d| d.fact.contains("$Data 11"))
        .unwrap_or_else(|| panic!("no D11 fact: {facts:?}"));
    assert!(d11.fact.contains("City, Name"), "{}", d11.fact);
    assert!(d11.fact.contains("\"contact\""), "{}", d11.fact);

    let (class, storage_name, nodes) = load_fixture("Member");
    let facts = derived_facts(&from_tree(&class, &storage_name, &nodes));
    assert!(
        !facts.iter().any(|d| d.fact.contains("$Data 11")),
        "{facts:?}"
    );
}

#[test]
fn test_nesting_fact_warns_the_parent_about_delete_t040() {
    use iris_agentic_dev_core::tools::storage_map::model::nesting_facts;
    let models: Vec<_> = ["Loan", "LoanEvent", "Member"]
        .iter()
        .map(|n| {
            let (c, s, nodes) = load_fixture(n);
            from_tree(&c, &s, &nodes)
        })
        .collect();
    let facts = nesting_facts(&models);
    assert_eq!(facts.len(), 1, "{facts:?}");
    let (parent, fact) = &facts[0];
    assert!(parent.starts_with("IadSm172.Loan."), "{parent}");
    assert!(fact.fact.contains("SQL DELETE"), "{}", fact.fact);
    assert!(fact.fact.contains("IadSm172.LoanEvent."), "{}", fact.fact);
}

#[test]
fn test_nested_maps_loan_loanevent_t040() {
    let (loan_class, loan_storage, loan_nodes) = load_fixture("Loan");
    let (event_class, event_storage, event_nodes) = load_fixture("LoanEvent");

    let loan_model = from_tree(&loan_class, &loan_storage, &loan_nodes);
    let event_model = from_tree(&event_class, &event_storage, &event_nodes);

    let models = vec![loan_model, event_model];
    let nested = nested_maps(&models);

    // LoanEvent is nested inside Loan
    assert_eq!(nested.len(), 1);
    assert_eq!(nested[0].0, "IadSm172.LoanEvent.Master");
    assert_eq!(nested[0].1, "IadSm172.Loan.Master");
}

#[test]
fn test_nested_maps_member_not_nested_t040() {
    let (class, storage_name, nodes) = load_fixture("Member");
    let model = from_tree(&class, &storage_name, &nodes);

    let models = vec![model];
    let nested = nested_maps(&models);

    // Member is not nested
    assert_eq!(nested.len(), 0);
}

// ───────────────────────────────────────────────────────────────────────────
// T041: Map and storage versions
// ───────────────────────────────────────────────────────────────────────────

#[test]
fn test_versions_stable_across_parses_t041() {
    let (class, storage_name, nodes1) = load_fixture("Member");
    let model1 = from_tree(&class, &storage_name, &nodes1);

    // Parse again
    let (_, _, nodes2) = load_fixture("Member");
    let model2 = from_tree(&class, &storage_name, &nodes2);

    // Versions should be identical
    assert_eq!(model1.storage.version, model2.storage.version);
    assert_eq!(
        model1.storage.maps[0].version,
        model2.storage.maps[0].version
    );
}

#[test]
fn test_version_changes_when_content_changes_t041() {
    let (class, storage_name, nodes) = load_fixture("Member");
    let model1 = from_tree(&class, &storage_name, &nodes);
    let version1 = model1.storage.version.clone();

    // Modify the tree slightly
    let mut nodes_modified = nodes.clone();
    if let Some(sqlmap) = nodes_modified.iter_mut().find(|n| n.tag == "SQLMap") {
        if let Some(ref mut children) = sqlmap.children {
            children.push(Node {
                tag: "TestField".to_string(),
                name: None,
                value: Some("test".to_string()),
                children: None,
            });
        }
    }

    let model2 = from_tree(&class, &storage_name, &nodes_modified);
    let version2 = model2.storage.version;

    // Version should change
    assert_ne!(version1, version2);
}

// ───────────────────────────────────────────────────────────────────────────
// T042: Default storage
// ───────────────────────────────────────────────────────────────────────────

#[test]
fn test_default_storage_t042() {
    let (class, storage_name, nodes) = load_fixture("DefaultBook");
    let model = from_tree(&class, &storage_name, &nodes);

    assert!(!model.storage.editable);
    assert_eq!(
        model.storage.not_editable_reason,
        Some("default storage".to_string())
    );
    assert!(model.storage.default_storage.is_some());

    let default = model.storage.default_storage.as_ref().unwrap();
    // The data_location is ^IadSm172.DefaultBookD, nodes contain DefaultBookDefaultData
    assert!(default.data_location.contains("DefaultBook"));
    assert!(!default.nodes.is_empty());
    assert_eq!(default.nodes[0].value_name, "DefaultBookDefaultData");
}

/// FR-003: the writable fact comes from the master map wherever it sorts. IRIS stores maps by
/// name, so an index map named before `Master` (`CityIdx`, `BadgeExtent`) comes first.
#[test]
fn test_derived_writable_fact_follows_the_master_map_not_the_first_map_t040() {
    for name in ["MemberByCity", "Badge", "Member"] {
        let (class, storage, nodes) = load_fixture(name);
        let m = from_tree(&class, &storage, &nodes);
        assert!(!m.storage.maps.is_empty(), "{name}");
        let facts: Vec<String> = derived_facts(&m)
            .into_iter()
            .filter(|d| d.path == "/storage" && d.fact.starts_with("table is "))
            .map(|d| d.fact)
            .collect();
        assert_eq!(
            facts,
            ["table is writable: master global starts with ^, $, or @"],
            "{name}"
        );
    }
}
