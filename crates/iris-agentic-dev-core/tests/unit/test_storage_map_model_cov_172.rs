//! Coverage tests for model.rs uncovered lines.

use iris_agentic_dev_core::tools::storage_map::model::*;
use iris_agentic_dev_core::tools::storage_map::tree::*;
use serde_json::json;

#[test]
fn test_from_tree_with_extra_elements() {
    // Lines 222-227: ExtraElement handling
    let nodes = vec![
        Node {
            tag: "Type".to_string(),
            name: None,
            value: Some("%Storage.SQL".to_string()),
            children: None,
        },
        Node {
            tag: "UnknownElement".to_string(),
            name: Some("test".to_string()),
            value: Some("value".to_string()),
            children: Some(Vec::new()),
        },
    ];

    let model = from_tree("Test.Class", "SQLStorage", &nodes);
    assert!(!model.storage.extra.is_empty());
    assert_eq!(model.storage.extra[0].tag, "UnknownElement");
    assert_eq!(model.storage.extra[0].name, Some("test".to_string()));
}

#[test]
fn test_from_tree_with_subscript_extras() {
    // Lines 331, 451-457: Subscript with warnings and extras
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
                    tag: "Subscript".to_string(),
                    name: Some("NotANumber".to_string()), // Triggers warning - line 331
                    children: Some(vec![
                        Node {
                            tag: "Expression".to_string(),
                            name: None,
                            value: Some("{ID}".to_string()),
                            children: None,
                        },
                        Node {
                            tag: "Accessvar".to_string(),
                            name: Some("1".to_string()),
                            children: Some(vec![
                                Node {
                                    tag: "Code".to_string(),
                                    name: None,
                                    value: Some("test code".to_string()), // Lines 495, 498
                                    children: None,
                                },
                                Node {
                                    tag: "Variable".to_string(),
                                    name: None,
                                    value: Some("testVar".to_string()),
                                    children: None,
                                },
                            ]),
                            value: None,
                        },
                        Node {
                            tag: "UnknownChild".to_string(), // Extra - lines 451-457
                            name: None,
                            value: Some("extra".to_string()),
                            children: None,
                        },
                    ]),
                    value: None,
                },
            ]),
            value: None,
        },
    ];

    let model = from_tree("Test.Class", "SQLStorage", &nodes);
    assert!(!model.derived.is_empty());
    assert!(model.derived[0].fact.contains("non-integer"));
}

#[test]
fn test_from_tree_with_retrieval_code() {
    // Lines 538-547: DataField with RetrievalCode and extras
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
                    children: Some(vec![
                        Node {
                            tag: "RetrievalCode".to_string(), // Line 538
                            name: None,
                            value: Some("some code".to_string()),
                            children: None,
                        },
                        Node {
                            tag: "UnknownTag".to_string(), // Lines 538-544
                            name: None,
                            value: Some("extra".to_string()),
                            children: None,
                        },
                    ]),
                    value: None,
                },
            ]),
            value: None,
        },
    ];

    let model = from_tree("Test.Class", "SQLStorage", &nodes);
    let map = &model.storage.maps[0];
    let field = map.data.get("ComputedField").unwrap();
    assert_eq!(field.retrieval_code, Some("some code".to_string()));
    assert!(field.derived.computed);
    assert!(!field.derived.filed);
}

#[test]
fn test_from_tree_with_row_id_spec() {
    // Lines 586-612: RowIdSpec parsing
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
                    tag: "RowIdSpec".to_string(),
                    name: Some("1".to_string()),
                    children: Some(vec![
                        Node {
                            tag: "Field".to_string(), // Line 609
                            name: None,
                            value: Some("ID".to_string()),
                            children: None,
                        },
                        Node {
                            tag: "Expression".to_string(), // Line 612
                            name: None,
                            value: Some("{ID}".to_string()),
                            children: None,
                        },
                    ]),
                    value: None,
                },
            ]),
            value: None,
        },
    ];

    let model = from_tree("Test.Class", "SQLStorage", &nodes);
    let map = &model.storage.maps[0];
    assert_eq!(map.row_id.len(), 1);
    assert_eq!(map.row_id[0].field, Some("ID".to_string()));
    assert_eq!(map.row_id[0].expression, Some("{ID}".to_string()));
}

#[test]
fn test_from_tree_with_default_storage() {
    // Lines 639-685: Default storage parsing
    let nodes = vec![
        Node {
            tag: "Type".to_string(),
            name: None,
            value: Some("%Storage.SQL".to_string()),
            children: None,
        },
        Node {
            tag: "DataLocation".to_string(),
            name: None,
            value: Some("^TestD".to_string()),
            children: None,
        },
        Node {
            tag: "IdLocation".to_string(),
            name: None,
            value: Some("^TestI".to_string()),
            children: None,
        },
        Node {
            tag: "IndexLocation".to_string(),
            name: None,
            value: Some("^TestX".to_string()),
            children: None,
        },
        Node {
            tag: "StreamLocation".to_string(),
            name: None,
            value: Some("^TestS".to_string()),
            children: None,
        },
        Node {
            tag: "Data".to_string(),
            name: Some("1".to_string()),
            children: Some(vec![Node {
                tag: "Value".to_string(),
                name: Some("1".to_string()),
                children: Some(vec![Node {
                    tag: "Value".to_string(), // Line 668
                    name: None,
                    value: Some("Name".to_string()),
                    children: None,
                }]),
                value: None,
            }]),
            value: None,
        },
    ];

    let model = from_tree("Test.Class", "SQLStorage", &nodes);
    assert!(model.storage.default_storage.is_some());
    let ds = model.storage.default_storage.as_ref().unwrap();
    assert_eq!(ds.data_location, "^TestD");
    assert_eq!(ds.id_location, "^TestI");
    assert_eq!(ds.index_location, "^TestX");
    assert_eq!(ds.stream_location, "^TestS");
    assert_eq!(ds.nodes.len(), 1);
    assert_eq!(ds.nodes[0].slots.len(), 1);
    assert_eq!(ds.nodes[0].slots[0].property, "Name");
}

#[test]
fn test_derived_facts_master_global_prefix() {
    // Lines 730-743: Master global writable check
    let storage = SqlStorage {
        name: "SQLStorage".to_string(),
        r#type: "%Storage.SQL".to_string(),
        version: "abc".to_string(),
        editable: true,
        not_editable_reason: None,
        keywords: std::collections::BTreeMap::new(),
        description: None,
        maps: vec![
            SqlMap {
                name: "Master".to_string(),
                version: "abc".to_string(),
                r#type: "data".to_string(),
                master: true,
                used: true,
                global: "$ZTEST".to_string(), // Starts with $ - writable
                subscripts: vec![],
                data: std::collections::BTreeMap::new(),
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
            },
            SqlMap {
                name: "ReadOnly".to_string(),
                version: "abc".to_string(),
                r#type: "data".to_string(),
                master: false,
                used: false,
                global: "PlainGlobal".to_string(), // Doesn't start with ^/$/@ - read-only
                subscripts: vec![],
                data: std::collections::BTreeMap::new(),
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
            },
        ],
        default_storage: None,
        extra: vec![],
    };

    let model = StorageModel {
        model_version: MODEL_VERSION,
        class: "Test.Class".to_string(),
        storage,
        derived: vec![],
    };

    let facts = derived_facts(&model);

    // Should have writable fact for first map
    assert!(facts.iter().any(|f| f.fact.contains("table is writable")));
}

#[test]
fn test_nested_maps_detection() {
    // Line 846: nested.push
    let models = vec![
        StorageModel {
            model_version: MODEL_VERSION,
            class: "Test.Parent".to_string(),
            storage: SqlStorage {
                name: "SQLStorage".to_string(),
                r#type: "%Storage.SQL".to_string(),
                version: "abc".to_string(),
                editable: true,
                not_editable_reason: None,
                keywords: std::collections::BTreeMap::new(),
                description: None,
                maps: vec![SqlMap {
                    name: "Master".to_string(),
                    version: "abc".to_string(),
                    r#type: "data".to_string(),
                    master: true,
                    used: true,
                    global: "^Test".to_string(),
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
                    data: std::collections::BTreeMap::new(),
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
                }],
                default_storage: None,
                extra: vec![],
            },
            derived: vec![],
        },
        StorageModel {
            model_version: MODEL_VERSION,
            class: "Test.Child".to_string(),
            storage: SqlStorage {
                name: "SQLStorage".to_string(),
                r#type: "%Storage.SQL".to_string(),
                version: "abc".to_string(),
                editable: true,
                not_editable_reason: None,
                keywords: std::collections::BTreeMap::new(),
                description: None,
                maps: vec![SqlMap {
                    name: "Master".to_string(),
                    version: "abc".to_string(),
                    r#type: "data".to_string(),
                    master: true,
                    used: true,
                    global: "^Test".to_string(),
                    subscripts: vec![
                        Subscript {
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
                        },
                        Subscript {
                            level: 2,
                            expression: "\"child\"".to_string(), // Literal - nested
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
                        },
                    ],
                    data: std::collections::BTreeMap::new(),
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
                }],
                default_storage: None,
                extra: vec![],
            },
            derived: vec![],
        },
    ];

    let nested = nested_maps(&models);
    assert_eq!(nested.len(), 1);
    assert_eq!(nested[0].0, "Test.Child.Master");
    assert_eq!(nested[0].1, "Test.Parent.Master");
}

#[test]
fn test_map_node_with_row_id() {
    // Line 921: RowIdSpec handling
    let map_json = json!({
        "name": "Master",
        "type": "data",
        "global": "^Test",
        "subscripts": [],
        "data": {},
        "row_id": [
            {
                "position": 1,
                "field": "ID"
            }
        ]
    });

    let node = map_node(&map_json).unwrap();
    assert_eq!(node.tag, "SQLMap");
    assert_eq!(node.name, Some("Master".to_string()));

    let children = node.children.as_ref().unwrap();
    assert!(children.iter().any(|c| c.tag == "RowIdSpec"));
}

#[test]
fn test_map_node_with_invalid_conditions() {
    // Lines 973, 980: Invalidcondition handling
    let map_json = json!({
        "name": "Master",
        "type": "data",
        "global": "^Test",
        "subscripts": [
            {
                "level": 1,
                "expression": "{ID}",
                "invalid_conditions": [
                    {
                        "level": 1,
                        "expression": "ID = \"\""
                    }
                ]
            }
        ],
        "data": {}
    });

    let node = map_node(&map_json).unwrap();
    let children = node.children.as_ref().unwrap();
    let subscript = children.iter().find(|c| c.tag == "Subscript").unwrap();
    let sub_children = subscript.children.as_ref().unwrap();
    assert!(sub_children.iter().any(|c| c.tag == "Invalidcondition"));
}

#[test]
fn test_data_node_name_mismatch() {
    // Lines 1000, 1003: Name mismatch check
    let map_json = json!({
        "name": "Master",
        "type": "data",
        "global": "^Test",
        "subscripts": [],
        "data": {
            "CorrectName": {
                "name": "WrongName"
            }
        }
    });

    let result = map_node(&map_json);
    assert!(result.is_err());
    assert!(result.unwrap_err().contains("differs from its key"));
}

#[test]
fn test_extra_nodes_error_handling() {
    // Lines 1064-1073: extra_nodes error handling
    let map_json = json!({
        "name": "Master",
        "type": "data",
        "global": "^Test",
        "subscripts": [],
        "data": {},
        "extra": [
            {
                "tag": "ExtraTag",
                "name": "test",
                "value": "val"
            },
            {
                // Missing tag - should error
                "name": "badnode"
            }
        ]
    });

    let err = map_node(&map_json).expect_err("the second extra has no tag");
    assert!(err.ends_with("/extra/1: no `tag`"), "{err}");
}

#[test]
fn test_list_error_handling() {
    // Line 1130: list function error handling
    let map_json = json!({
        "name": "Master",
        "type": "data",
        "global": "^Test",
        "subscripts": "not a list",
        "data": {}
    });

    let result = map_node(&map_json);
    assert!(result.is_err());
    assert!(result.unwrap_err().contains("expected a list"));
}

#[test]
fn test_element_node_numbered_variants() {
    // Lines 1162-1165: element_node for numbered elements

    // RowIdSpec
    let row_id_json = json!({
        "field": "ID"
    });
    let node = element_node("RowIdSpec", "1", &row_id_json).unwrap();
    assert_eq!(node.tag, "RowIdSpec");
    assert_eq!(node.name, Some("1".to_string()));

    // Accessvar
    let accessvar_json = json!({
        "code": "test",
        "variable": "var"
    });
    let node = element_node("Accessvar", "1", &accessvar_json).unwrap();
    assert_eq!(node.tag, "Accessvar");
    assert_eq!(node.name, Some("1".to_string()));

    // Invalidcondition
    let invalid_json = json!({
        "expression": "test"
    });
    let node = element_node("Invalidcondition", "1", &invalid_json).unwrap();
    assert_eq!(node.tag, "Invalidcondition");
    assert_eq!(node.name, Some("1".to_string()));
}
