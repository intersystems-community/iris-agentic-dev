//! Coverage tests for tree.rs uncovered lines.

use iris_agentic_dev_core::tools::storage_map::tree::*;

#[test]
fn test_is_canonical_number_edge_cases() {
    // Line 56: Minus with no following digits
    assert!(!is_canonical_number("-"));

    // Lines 65, 73, 77: Decimal edge cases
    assert!(!is_canonical_number(".")); // Just a dot
    assert!(!is_canonical_number(".0")); // Trailing zero after dot
    assert!(is_canonical_number("-.5")); // Valid negative decimal

    // Line 102: Non-decimal-point after integer
    assert!(!is_canonical_number("123a"));

    // Line 117: No digits after decimal point
    assert!(!is_canonical_number("123."));
}

#[test]
fn test_ordered_float_through_order_key() {
    // Lines 142-155: OrderedFloat PartialOrd and Ord implementations
    // Tested indirectly through order_key() which creates OrderedFloats
    let key1 = order_key(Some("1.5"));
    let key2 = order_key(Some("2.5"));
    let key3 = order_key(Some("1.5"));

    assert!(key1 < key2);
    assert!(key2 > key1);
    assert_eq!(key1, key3);
}

#[test]
fn test_parse_error_display() {
    // Lines 188-197: Display and Error trait for ParseError
    let err = ParseError {
        offset: 42,
        context: "test context".to_string(),
        reason: "test reason".to_string(),
    };

    let display = format!("{}", err);
    assert!(display.contains("Parse error at offset 42"));
    assert!(display.contains("test reason"));
    assert!(display.contains("test context"));

    // Line 194: Error trait
    use std::error::Error;
    assert!(err.source().is_none());
}

#[test]
fn test_parse_body_missing_newline_between_elements() {
    // Lines 233-237: Missing newline between elements
    let text = "<Type>%Storage.SQL</Type><Global>^Test</Global>";
    let result = parse_body(text);
    assert!(result.is_err());
    let err = result.unwrap_err();
    assert!(err.reason.contains("expected newline between elements"));
}

#[test]
fn test_normalize_empty_leaf() {
    // Line 572: Dropping empty leaves
    let nodes = vec![
        Node {
            tag: "Type".to_string(),
            name: None,
            value: Some("".to_string()), // Empty value
            children: None,
        },
        Node {
            tag: "Global".to_string(),
            name: None,
            value: Some("^Test".to_string()),
            children: None,
        },
    ];

    let normalized = normalize(nodes);
    assert_eq!(normalized.len(), 1);
    assert_eq!(normalized[0].tag, "Global");
}

#[test]
fn test_split_class_no_storage_found() {
    // Lines 613-614: name_end not found
    let text = "Class Test.Sample\n{\nStorage ";
    let result = split_class(text);
    assert!(result.is_ok());
    assert!(result.unwrap().is_empty());
}

#[test]
fn test_split_class_no_opening_brace() {
    // Lines 622-623: Opening brace not found
    let text = "Class Test.Sample\n{\nStorage Default\nProperty Name As %String;\n}";
    let result = split_class(text);
    assert!(result.is_ok());
    assert!(result.unwrap().is_empty());
}

#[test]
fn test_split_class_unexpected_element() {
    // Lines 652-656: Expected newline and element or close brace
    let text = r#"Class Test.Sample
{

Storage Default
{
<Type>%Storage.SQL</Type>
unexpected text
}

}
"#;
    let result = split_class(text);
    assert!(result.is_err());
    let err = result.unwrap_err();
    assert!(err
        .reason
        .contains("expected newline and element or close brace"));
}

// Lines 284, 323-327, 372-376, 410, 432, 438-442, 447, 457-461, 500, 519:
// These are inside private functions (parse_element, parse_opening_tag, emit_nodes).
// They are exercised through integration tests that parse real Storage blocks.
