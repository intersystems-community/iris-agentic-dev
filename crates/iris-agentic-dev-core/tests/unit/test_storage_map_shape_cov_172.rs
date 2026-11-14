//! Coverage tests for shape.rs uncovered lines.

use iris_agentic_dev_core::tools::storage_map::shape::*;
use std::collections::BTreeMap;

// Note: Lines 102-108 (add() method with values) are pub(crate) and called
// during sampling, which is tested through integration tests with live IRIS.

#[test]
fn test_compare_map_computed_field_skipped() {
    use iris_agentic_dev_core::tools::storage_map::model::*;

    // Test that a computed field (with retrieval_code) is skipped - line 151
    let mut data = BTreeMap::new();
    data.insert(
        "ComputedField".to_string(),
        DataField {
            name: "ComputedField".to_string(),
            node: None,
            delimiter: Some("\"^\"".to_string()),
            piece: Some("1".to_string()),
            retrieval_code: Some("some code".to_string()), // Computed field - line 151
            extra: Vec::new(),
            derived: DataDerived {
                format: "delimited".to_string(),
                computed: true,
                filed: false,
            },
        },
    );
    // Add a non-computed field so there's something that reads the row node
    data.insert(
        "RealField".to_string(),
        DataField {
            name: "RealField".to_string(),
            node: None,
            delimiter: Some("\"^\"".to_string()),
            piece: Some("1".to_string()),
            retrieval_code: None,
            extra: Vec::new(),
            derived: DataDerived {
                format: "delimited".to_string(),
                computed: false,
                filed: true,
            },
        },
    );

    let map = SqlMap {
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
            access_vars: Vec::new(),
            invalid_conditions: Vec::new(),
            extra: Vec::new(),
        }],
        data,
        row_id: Vec::new(),
        condition: None,
        condition_fields: None,
        conditional_with_host_vars: None,
        population_type: None,
        population_pct: None,
        block_count: None,
        structure: None,
        row_reference: None,
        deferred: None,
        extra: Vec::new(),
    };

    let shape = ShapeReport {
        global: "^Test".to_string(),
        nodes_scanned: 1,
        capped: false,
        cap: Cap {
            nodes: 100,
            seconds: 10,
        },
        depths: vec![("1".to_string(), 1)].into_iter().collect(),
        data_states: BTreeMap::new(),
        sub_nodes: BTreeMap::new(),
        numeric_subscripts: BTreeMap::new(),
        formats: vec![Format {
            depth: 1,
            node: None,
            format: "delimited".to_string(),
            delimiter: Some("^".to_string()),
            pieces: Pieces { min: 1, max: 1 },
        }],
        values: None,
    };

    let mismatches = compare_map(&map, &shape);
    // Computed field is skipped, real field matches, so no mismatches
    assert!(mismatches.is_empty());
}

#[test]
fn test_compare_map_no_value_at_depth() {
    use iris_agentic_dev_core::tools::storage_map::model::*;

    let mut data = BTreeMap::new();
    data.insert(
        "Field1".to_string(),
        DataField {
            name: "Field1".to_string(),
            node: None,
            delimiter: Some("\"^\"".to_string()),
            piece: Some("1".to_string()),
            retrieval_code: None,
            extra: Vec::new(),
            derived: DataDerived {
                format: "delimited".to_string(),
                computed: false,
                filed: true,
            },
        },
    );

    let map = SqlMap {
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
            access_vars: Vec::new(),
            invalid_conditions: Vec::new(),
            extra: Vec::new(),
        }],
        data,
        row_id: Vec::new(),
        condition: None,
        condition_fields: None,
        conditional_with_host_vars: None,
        population_type: None,
        population_pct: None,
        block_count: None,
        structure: None,
        row_reference: None,
        deferred: None,
        extra: Vec::new(),
    };

    // Empty shape - no formats at the expected depth
    let shape = ShapeReport {
        global: "^Test".to_string(),
        nodes_scanned: 0,
        capped: false,
        cap: Cap {
            nodes: 100,
            seconds: 10,
        },
        depths: BTreeMap::new(),
        data_states: BTreeMap::new(),
        sub_nodes: BTreeMap::new(),
        numeric_subscripts: BTreeMap::new(),
        formats: Vec::new(), // No formats - triggers lines 168-174
        values: None,
    };

    let mismatches = compare_map(&map, &shape);
    assert_eq!(mismatches.len(), 1);
    assert_eq!(mismatches[0].path, "/maps/Master/data/Field1");
    assert_eq!(mismatches[0].expected, "a value at depth 1");
    assert_eq!(mismatches[0].seen, "no value there");
}

#[test]
fn test_compare_map_delimiter_mismatch() {
    use iris_agentic_dev_core::tools::storage_map::model::*;

    let mut data = BTreeMap::new();
    data.insert(
        "Field1".to_string(),
        DataField {
            name: "Field1".to_string(),
            node: None,
            delimiter: Some("\"^\"".to_string()),
            piece: Some("1".to_string()),
            retrieval_code: None,
            extra: Vec::new(),
            derived: DataDerived {
                format: "delimited".to_string(),
                computed: false,
                filed: true,
            },
        },
    );

    let map = SqlMap {
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
            access_vars: Vec::new(),
            invalid_conditions: Vec::new(),
            extra: Vec::new(),
        }],
        data,
        row_id: Vec::new(),
        condition: None,
        condition_fields: None,
        conditional_with_host_vars: None,
        population_type: None,
        population_pct: None,
        block_count: None,
        structure: None,
        row_reference: None,
        deferred: None,
        extra: Vec::new(),
    };

    // Shape with different delimiter - triggers lines 188-193
    let shape = ShapeReport {
        global: "^Test".to_string(),
        nodes_scanned: 1,
        capped: false,
        cap: Cap {
            nodes: 100,
            seconds: 10,
        },
        depths: vec![("1".to_string(), 1)].into_iter().collect(),
        data_states: BTreeMap::new(),
        sub_nodes: BTreeMap::new(),
        numeric_subscripts: BTreeMap::new(),
        formats: vec![Format {
            depth: 1,
            node: None,
            format: "delimited".to_string(),
            delimiter: Some("|".to_string()), // Different delimiter
            pieces: Pieces { min: 1, max: 1 },
        }],
        values: None,
    };

    let mismatches = compare_map(&map, &shape);
    assert_eq!(mismatches.len(), 1);
    assert!(mismatches[0].expected.contains("pieces delimited by \"^\""));
    assert!(mismatches[0].seen.contains("delimited by \"|\""));
}

#[test]
fn test_compare_map_list_vs_delimited() {
    use iris_agentic_dev_core::tools::storage_map::model::*;

    let mut data = BTreeMap::new();
    data.insert(
        "Field1".to_string(),
        DataField {
            name: "Field1".to_string(),
            node: None,
            delimiter: Some("\"^\"".to_string()),
            piece: Some("1".to_string()),
            retrieval_code: None,
            extra: Vec::new(),
            derived: DataDerived {
                format: "delimited".to_string(),
                computed: false,
                filed: true,
            },
        },
    );

    let map = SqlMap {
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
            access_vars: Vec::new(),
            invalid_conditions: Vec::new(),
            extra: Vec::new(),
        }],
        data,
        row_id: Vec::new(),
        condition: None,
        condition_fields: None,
        conditional_with_host_vars: None,
        population_type: None,
        population_pct: None,
        block_count: None,
        structure: None,
        row_reference: None,
        deferred: None,
        extra: Vec::new(),
    };

    // Shape with $LIST - triggers lines 194-197
    let shape = ShapeReport {
        global: "^Test".to_string(),
        nodes_scanned: 1,
        capped: false,
        cap: Cap {
            nodes: 100,
            seconds: 10,
        },
        depths: vec![("1".to_string(), 1)].into_iter().collect(),
        data_states: BTreeMap::new(),
        sub_nodes: BTreeMap::new(),
        numeric_subscripts: BTreeMap::new(),
        formats: vec![Format {
            depth: 1,
            node: None,
            format: "$LIST".to_string(),
            delimiter: None,
            pieces: Pieces { min: 2, max: 2 },
        }],
        values: None,
    };

    let mismatches = compare_map(&map, &shape);
    assert_eq!(mismatches.len(), 1);
    assert!(mismatches[0].expected.contains("pieces delimited by \"^\""));
    assert_eq!(mismatches[0].seen, "$LIST values");
}

#[test]
fn test_compare_map_no_delimiter_vs_delimited() {
    use iris_agentic_dev_core::tools::storage_map::model::*;

    let mut data = BTreeMap::new();
    data.insert(
        "Field1".to_string(),
        DataField {
            name: "Field1".to_string(),
            node: None,
            delimiter: None, // No delimiter - expect $LIST
            piece: Some("1".to_string()),
            retrieval_code: None,
            extra: Vec::new(),
            derived: DataDerived {
                format: "$LIST".to_string(),
                computed: false,
                filed: true,
            },
        },
    );

    let map = SqlMap {
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
            access_vars: Vec::new(),
            invalid_conditions: Vec::new(),
            extra: Vec::new(),
        }],
        data,
        row_id: Vec::new(),
        condition: None,
        condition_fields: None,
        conditional_with_host_vars: None,
        population_type: None,
        population_pct: None,
        block_count: None,
        structure: None,
        row_reference: None,
        deferred: None,
        extra: Vec::new(),
    };

    // Shape with delimited text - triggers lines 199-204
    let shape = ShapeReport {
        global: "^Test".to_string(),
        nodes_scanned: 1,
        capped: false,
        cap: Cap {
            nodes: 100,
            seconds: 10,
        },
        depths: vec![("1".to_string(), 1)].into_iter().collect(),
        data_states: BTreeMap::new(),
        sub_nodes: BTreeMap::new(),
        numeric_subscripts: BTreeMap::new(),
        formats: vec![Format {
            depth: 1,
            node: None,
            format: "delimited".to_string(),
            delimiter: Some("|".to_string()),
            pieces: Pieces { min: 1, max: 1 },
        }],
        values: None,
    };

    let mismatches = compare_map(&map, &shape);
    assert_eq!(mismatches.len(), 1);
    assert_eq!(mismatches[0].expected, "$LIST (no delimiter)");
    assert!(mismatches[0].seen.contains("text delimited by"));
}

#[test]
fn test_draft_with_special_node_names() {
    let report = ShapeReport {
        global: "^Test".to_string(),
        nodes_scanned: 2,
        capped: false,
        cap: Cap {
            nodes: 100,
            seconds: 10,
        },
        depths: vec![("1".to_string(), 2)].into_iter().collect(),
        data_states: BTreeMap::new(),
        sub_nodes: BTreeMap::new(),
        numeric_subscripts: BTreeMap::new(),
        formats: vec![
            Format {
                depth: 1,
                node: None,
                format: "$LIST".to_string(),
                delimiter: None,
                pieces: Pieces { min: 2, max: 2 },
            },
            Format {
                depth: 2,
                node: Some("\"123\"".to_string()), // Starts with digit - triggers line 314
                format: "$LIST".to_string(),
                delimiter: None,
                pieces: Pieces { min: 1, max: 1 },
            },
        ],
        values: None,
    };

    let (map, _guesses) = draft(&report);
    let data = map["data"].as_object().unwrap();

    // Node name starting with digit gets "Node" prefix
    assert!(data.keys().any(|k| k.starts_with("Node")));
}

#[test]
fn test_draft_with_list_delimiter() {
    let report = ShapeReport {
        global: "^Test".to_string(),
        nodes_scanned: 1,
        capped: false,
        cap: Cap {
            nodes: 100,
            seconds: 10,
        },
        depths: vec![("1".to_string(), 1)].into_iter().collect(),
        data_states: BTreeMap::new(),
        sub_nodes: BTreeMap::new(),
        numeric_subscripts: BTreeMap::new(),
        formats: vec![Format {
            depth: 1,
            node: None,
            format: "$LIST".to_string(),
            delimiter: None, // Triggers line 282 - None case
            pieces: Pieces { min: 1, max: 1 },
        }],
        values: None,
    };

    let (map, _guesses) = draft(&report);
    let data = map["data"].as_object().unwrap();
    let field = &data["Field1"];

    // $LIST format has no delimiter
    assert!(field.get("delimiter").is_none());
}
