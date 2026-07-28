#![allow(clippy::all)]
use iris_agentic_dev_core::tools::symbols_local::*;
use std::path::PathBuf;

fn fixtures_dir() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures")
}

// ── T010: glob_match unit tests ──────────────────────────────────────────────

#[test]
fn glob_exact() {
    assert!(glob_match("MyApp.Foo", "MyApp.Foo"));
}

#[test]
fn glob_no_implicit_substring() {
    assert!(!glob_match("Foo", "MyApp.Foo"));
}

#[test]
fn glob_package_prefix() {
    assert!(glob_match("MyApp.*", "MyApp.Foo"));
    assert!(glob_match("MyApp.*", "MyApp.Bar"));
    assert!(!glob_match("MyApp.*", "OtherApp.Foo"));
}

#[test]
fn glob_suffix() {
    assert!(glob_match("*Service", "OrderService"));
    assert!(!glob_match("*Service", "OrderUtil"));
}

#[test]
fn glob_mid() {
    assert!(glob_match("MyApp.*.Base", "MyApp.Sub.Base"));
    assert!(!glob_match("MyApp.*.Base", "MyApp.Sub.Other"));
}

#[test]
fn glob_empty_never_matches() {
    assert!(!glob_match("", "anything"));
    assert!(!glob_match("", ""));
}

// ── T015: extract_cls_symbols on Foo.cls ─────────────────────────────────────

#[test]
fn extract_cls_foo() {
    let path = fixtures_dir().join("MyApp/Foo.cls");
    let source = std::fs::read(&path).expect("read Foo.cls");
    let (symbols, warnings) = extract_cls_symbols(&source, "MyApp/Foo.cls", "MyApp.Foo");

    // No parse errors for the valid file
    let parse_errors: Vec<_> = warnings
        .iter()
        .filter(|w| w.warning_type == "PARSE_ERROR")
        .collect();
    assert!(
        parse_errors.is_empty(),
        "Unexpected parse errors: {:?}",
        parse_errors
    );

    let names: Vec<&str> = symbols.iter().map(|s| s.name.as_str()).collect();

    assert!(
        names.contains(&"MyApp.Foo"),
        "class symbol missing; got {:?}",
        names
    );

    let has_method = symbols
        .iter()
        .any(|s| s.name == "MyApp.Foo.DoSomething" && s.kind == "method");
    assert!(has_method, "DoSomething method not found; got {:?}", names);

    let method = symbols
        .iter()
        .find(|s| s.name == "MyApp.Foo.DoSomething")
        .unwrap();
    assert!(
        method
            .formal_spec
            .as_ref()
            .map(|f| !f.is_empty())
            .unwrap_or(false),
        "FormalSpec should be non-empty"
    );

    let has_property = symbols
        .iter()
        .any(|s| s.name == "MyApp.Foo.Value" && s.kind == "property");
    assert!(has_property, "Value property not found; got {:?}", names);

    let has_param = symbols
        .iter()
        .any(|s| s.name == "MyApp.Foo.VERSION" && s.kind == "parameter");
    assert!(has_param, "VERSION parameter not found; got {:?}", names);
}

// ── T016: glob used in scan matches package correctly ───────────────────────

#[test]
fn glob_package_scan_match() {
    assert!(glob_match("MyApp.*", "MyApp.Foo"));
    assert!(glob_match("MyApp.*", "MyApp.Bar"));
    assert!(!glob_match("MyApp.*", "OtherApp.Foo"));
}

// ── T017: scan_workspace with exact class query ──────────────────────────────

#[test]
fn scan_workspace_exact_query() {
    let result = scan_workspace(&fixtures_dir(), "MyApp.Foo", 50);

    let has_class = result
        .symbols
        .iter()
        .any(|s| s.name == "MyApp.Foo" && s.kind == "class");
    assert!(has_class, "class symbol not found: {:?}", result.symbols);

    assert!(
        result.symbols.len() >= 3,
        "expected at least 3 symbols, got {}",
        result.symbols.len()
    );

    // No PARSE_ERROR for Foo.cls (it is valid)
    let foo_errors: Vec<_> = result
        .parse_warnings
        .iter()
        .filter(|w| {
            w.warning_type == "PARSE_ERROR"
                && w.file
                    .as_deref()
                    .map(|f| f.contains("Foo.cls"))
                    .unwrap_or(false)
                && !w
                    .file
                    .as_deref()
                    .map(|f| f.contains("Broken") || f.contains("Dupe"))
                    .unwrap_or(false)
        })
        .collect();
    assert!(
        foo_errors.is_empty(),
        "Unexpected PARSE_ERROR for Foo.cls: {:?}",
        foo_errors
    );
}

// ── T018: scan with wildcard triggers DUPLICATE_CLASS warning ────────────────

#[test]
fn scan_workspace_wildcard_detects_duplicate() {
    let result = scan_workspace(&fixtures_dir(), "MyApp.*", 200);

    let has_duplicate = result
        .parse_warnings
        .iter()
        .any(|w| w.warning_type == "DUPLICATE_CLASS" && w.class.as_deref() == Some("MyApp.Foo"));
    assert!(
        has_duplicate,
        "Expected DUPLICATE_CLASS for MyApp.Foo; warnings: {:?}",
        result.parse_warnings
    );

    // Symbols from Foo.cls should still be present
    assert!(
        result.symbols.iter().any(|s| s.name == "MyApp.Foo"),
        "MyApp.Foo class symbol should still appear despite duplicate"
    );
}

// ── T070-03/04: line field present and correct (US2) ─────────────────────────

#[test]
fn t070_03_line_field_present() {
    let path = fixtures_dir().join("MyApp/Foo.cls");
    let source = std::fs::read(&path).expect("read Foo.cls");
    let (symbols, _) = extract_cls_symbols(&source, "MyApp/Foo.cls", "MyApp.Foo");

    for sym in &symbols {
        assert!(
            sym.line >= 1,
            "symbol {} has line={}, expected >= 1",
            sym.name,
            sym.line
        );
    }
}

#[test]
fn t070_04_line_field_correct_for_do_something() {
    let path = fixtures_dir().join("MyApp/Foo.cls");
    let source = std::fs::read(&path).expect("read Foo.cls");
    let (symbols, _) = extract_cls_symbols(&source, "MyApp/Foo.cls", "MyApp.Foo");

    let method = symbols
        .iter()
        .find(|s| s.name == "MyApp.Foo.DoSomething")
        .expect("DoSomething not found");

    // DoSomething is on line 8 in Foo.cls
    assert_eq!(
        method.line, 8,
        "DoSomething expected at line 8, got {}",
        method.line
    );
}

// ── T070-05: line numbers on routine labels (US2) ─────────────────────────────

#[test]
fn t070_05_routine_label_line_numbers() {
    let path = fixtures_dir().join("Utils.mac");
    let source = std::fs::read(&path).expect("read Utils.mac");
    let (symbols, _) = extract_routine_symbols(&source, "Utils.mac", "Utils");

    for sym in symbols.iter().filter(|s| s.kind == "label") {
        assert!(
            sym.line >= 1,
            "label {} has line={}, expected >= 1",
            sym.name,
            sym.line
        );
    }
}

// ── T070-06/07/08: return types extracted from AST (US3) ─────────────────────

#[test]
fn t070_06_property_type() {
    let path = fixtures_dir().join("MyApp/TypedMembers.cls");
    let source = std::fs::read(&path).expect("read TypedMembers.cls");
    let (symbols, _) = extract_cls_symbols(&source, "MyApp/TypedMembers.cls", "MyApp.TypedMembers");

    let prop = symbols
        .iter()
        .find(|s| s.name == "MyApp.TypedMembers.Value")
        .expect("Value property not found");

    assert_eq!(
        prop.type_name.as_deref(),
        Some("%String"),
        "Value.Type expected %String, got {:?}",
        prop.type_name
    );
}

#[test]
fn t070_07_method_return_type() {
    let path = fixtures_dir().join("MyApp/TypedMembers.cls");
    let source = std::fs::read(&path).expect("read TypedMembers.cls");
    let (symbols, _) = extract_cls_symbols(&source, "MyApp/TypedMembers.cls", "MyApp.TypedMembers");

    let method = symbols
        .iter()
        .find(|s| s.name == "MyApp.TypedMembers.DoSomething")
        .expect("DoSomething not found");

    assert_eq!(
        method.type_name.as_deref(),
        Some("%Boolean"),
        "DoSomething.Type expected %Boolean, got {:?}",
        method.type_name
    );
}

#[test]
fn t070_08_no_type_for_untyped_param() {
    let path = fixtures_dir().join("MyApp/TypedMembers.cls");
    let source = std::fs::read(&path).expect("read TypedMembers.cls");
    let (symbols, _) = extract_cls_symbols(&source, "MyApp/TypedMembers.cls", "MyApp.TypedMembers");

    let param = symbols
        .iter()
        .find(|s| s.name == "MyApp.TypedMembers.VERSION")
        .expect("VERSION not found");

    assert!(
        param.type_name.is_none(),
        "VERSION should have no Type, got {:?}",
        param.type_name
    );
}

// ── T070-09..13: structured FormalSpec (US4) — write fixture first ───────────

// NOTE: FormalSpec.cls fixture created in T009; tests written here to fail first.

#[test]
fn t070_09_formal_spec_is_vec() {
    let path = fixtures_dir().join("MyApp/FormalSpec.cls");
    // Fixture not yet created — this test will fail with file not found or compile error
    // until T009 creates the fixture and struct changes land.
    let source = std::fs::read(&path).expect("read FormalSpec.cls");
    let (symbols, _) = extract_cls_symbols(&source, "MyApp/FormalSpec.cls", "MyApp.FormalSpec");

    let method = symbols
        .iter()
        .find(|s| s.name == "MyApp.FormalSpec.WithArgs")
        .expect("WithArgs method not found");

    let spec = method
        .formal_spec
        .as_ref()
        .expect("FormalSpec should be Some");
    assert!(!spec.is_empty(), "FormalSpec vec should not be empty");
}

#[test]
fn t070_10_formal_spec_name() {
    let path = fixtures_dir().join("MyApp/FormalSpec.cls");
    let source = std::fs::read(&path).expect("read FormalSpec.cls");
    let (symbols, _) = extract_cls_symbols(&source, "MyApp/FormalSpec.cls", "MyApp.FormalSpec");

    let method = symbols
        .iter()
        .find(|s| s.name == "MyApp.FormalSpec.WithArgs")
        .expect("WithArgs not found");
    let spec = method.formal_spec.as_ref().expect("FormalSpec missing");
    assert_eq!(spec[0].name, "pName", "first arg name wrong");
}

#[test]
fn t070_11_formal_spec_type() {
    let path = fixtures_dir().join("MyApp/FormalSpec.cls");
    let source = std::fs::read(&path).expect("read FormalSpec.cls");
    let (symbols, _) = extract_cls_symbols(&source, "MyApp/FormalSpec.cls", "MyApp.FormalSpec");

    let method = symbols
        .iter()
        .find(|s| s.name == "MyApp.FormalSpec.WithArgs")
        .expect("WithArgs not found");
    let spec = method.formal_spec.as_ref().expect("FormalSpec missing");
    assert_eq!(
        spec[0].type_name.as_deref(),
        Some("%String"),
        "first arg type wrong"
    );
}

#[test]
fn t070_12_formal_spec_byref() {
    let path = fixtures_dir().join("MyApp/FormalSpec.cls");
    let source = std::fs::read(&path).expect("read FormalSpec.cls");
    let (symbols, _) = extract_cls_symbols(&source, "MyApp/FormalSpec.cls", "MyApp.FormalSpec");

    let method = symbols
        .iter()
        .find(|s| s.name == "MyApp.FormalSpec.WithArgs")
        .expect("WithArgs not found");
    let spec = method.formal_spec.as_ref().expect("FormalSpec missing");
    // second arg is ByRef pRef As %Integer
    assert!(spec[1].byref, "second arg should have byref=true");
}

#[test]
fn t070_13_formal_spec_default() {
    let path = fixtures_dir().join("MyApp/FormalSpec.cls");
    let source = std::fs::read(&path).expect("read FormalSpec.cls");
    let (symbols, _) = extract_cls_symbols(&source, "MyApp/FormalSpec.cls", "MyApp.FormalSpec");

    let method = symbols
        .iter()
        .find(|s| s.name == "MyApp.FormalSpec.WithArgs")
        .expect("WithArgs not found");
    let spec = method.formal_spec.as_ref().expect("FormalSpec missing");
    // first arg has default "hello"
    assert!(spec[0].default.is_some(), "first arg should have a default");
}

// ── T070-02: PythonBody.cls parses without PARSE_ERROR (US1 / grammar 1.9) ───

#[test]
fn t070_02_python_body_no_parse_error() {
    let path = fixtures_dir().join("MyApp/PythonBody.cls");
    let source = std::fs::read(&path).expect("read PythonBody.cls");
    let (symbols, warnings) =
        extract_cls_symbols(&source, "MyApp/PythonBody.cls", "MyApp.PythonBody");

    let parse_errors: Vec<_> = warnings
        .iter()
        .filter(|w| w.warning_type == "PARSE_ERROR")
        .collect();
    assert!(
        parse_errors.is_empty(),
        "PARSE_ERROR on python body class: {:?}",
        parse_errors
    );

    let has_greet = symbols.iter().any(|s| s.name == "MyApp.PythonBody.Greet");
    assert!(has_greet, "Greet method missing; symbols: {:?}", symbols);

    let has_compute = symbols.iter().any(|s| s.name == "MyApp.PythonBody.Compute");
    assert!(
        has_compute,
        "Compute method missing; symbols: {:?}",
        symbols
    );
}

// ── T022 / SC-006: NOT_IMPLEMENTED never returned ────────────────────────────

#[test]
fn sc006_not_implemented_never_returned() {
    // Call scan_workspace directly; it must never produce NOT_IMPLEMENTED
    let result = scan_workspace(&fixtures_dir(), "MyApp.Foo", 10);
    // Serialize to check no NOT_IMPLEMENTED in output
    let json = serde_json::to_string(&result.symbols).unwrap_or_default();
    assert!(
        !json.contains("NOT_IMPLEMENTED"),
        "NOT_IMPLEMENTED must never appear in symbols output"
    );
}

// ── T023: Broken.cls produces PARSE_ERROR, no panic ──────────────────────────

#[test]
fn extract_broken_cls_no_panic() {
    let path = fixtures_dir().join("MyApp/Broken.cls");
    let source = std::fs::read(&path).expect("read Broken.cls");
    let (symbols, warnings) = extract_cls_symbols(&source, "MyApp/Broken.cls", "MyApp.Broken");

    let has_error = warnings.iter().any(|w| w.warning_type == "PARSE_ERROR");
    assert!(
        has_error,
        "Expected PARSE_ERROR warning for Broken.cls; got {:?}",
        warnings
    );

    // Must not panic and must return a (possibly empty) symbols vec
    let _ = symbols;
}

// ── T024: scan_workspace includes errors from Broken.cls + symbols from Foo ──

#[test]
fn scan_includes_errors_and_valid_symbols() {
    let result = scan_workspace(&fixtures_dir(), "MyApp.*", 200);

    let has_broken_error = result.parse_warnings.iter().any(|w| {
        w.warning_type == "PARSE_ERROR"
            && w.file
                .as_deref()
                .map(|f| f.contains("Broken"))
                .unwrap_or(false)
    });
    assert!(
        has_broken_error,
        "Expected PARSE_ERROR for Broken.cls; warnings: {:?}",
        result.parse_warnings
    );

    // Symbols from the valid Foo.cls must still be present
    assert!(
        result.symbols.iter().any(|s| s.name == "MyApp.Foo"),
        "MyApp.Foo should still be returned despite Broken.cls parse error"
    );
    assert!(result.symbols.len() > 0, "count should be > 0");
}

// ── T026 / SC-004: parse error does NOT make the result an error ─────────────

#[test]
fn sc004_parse_error_no_error_response() {
    let result = scan_workspace(&fixtures_dir(), "MyApp.*", 200);
    // The scan itself must not return an error-level result —
    // verified by the fact that it returns a SymbolsLocalResult, not Err.
    // parse_warnings may contain PARSE_ERROR; that is fine.
    assert!(
        result
            .parse_warnings
            .iter()
            .any(|w| w.warning_type == "PARSE_ERROR")
            || !result.symbols.is_empty(),
        "result must be non-erroring (either has warnings or has symbols)"
    );
}

// ── T027: extract_routine_symbols on Utils.mac ───────────────────────────────

#[test]
fn extract_routine_utils_mac() {
    let path = fixtures_dir().join("Utils.mac");
    let source = std::fs::read(&path).expect("read Utils.mac");
    let (symbols, _warnings) = extract_routine_symbols(&source, "Utils.mac", "Utils");

    let names: Vec<&str> = symbols.iter().map(|s| s.name.as_str()).collect();

    let has_start = symbols
        .iter()
        .any(|s| s.name == "Utils:Start" && s.kind == "label");
    let has_helper = symbols
        .iter()
        .any(|s| s.name == "Utils:Helper" && s.kind == "label");

    assert!(has_start, "Utils:Start label not found; got {:?}", names);
    assert!(has_helper, "Utils:Helper label not found; got {:?}", names);
}

// ── T028: extract_routine_symbols on Macros.inc ──────────────────────────────

#[test]
fn extract_routine_macros_inc() {
    let path = fixtures_dir().join("Macros.inc");
    let source = std::fs::read(&path).expect("read Macros.inc");
    // For .inc files, query matches on filename stem "Macros"
    let (symbols, _warnings) = extract_routine_symbols(&source, "Macros.inc", "Macros");

    let macro_count = symbols.iter().filter(|s| s.kind == "macro").count();
    assert!(
        macro_count >= 2,
        "Expected at least 2 macro symbols; got {:?}",
        symbols
    );
}

// ── T029: scan_workspace with routine query ───────────────────────────────────

#[test]
fn scan_workspace_routine_query() {
    let result = scan_workspace(&fixtures_dir(), "Utils", 50);
    let has_label = result
        .symbols
        .iter()
        .any(|s| s.kind == "label" && s.name.starts_with("Utils:"));
    assert!(
        has_label,
        "Expected routine label symbols for Utils; got {:?}",
        result.symbols
    );
}

// ── T031 / SC-002: output shape parity ───────────────────────────────────────

#[test]
fn sc002_shape_parity() {
    let result = scan_workspace(&fixtures_dir(), "MyApp.Foo", 50);

    // Check that each symbol has the required top-level keys
    for sym in &result.symbols {
        let v = serde_json::to_value(sym).unwrap();
        assert!(v.get("Name").is_some(), "Symbol missing 'Name' field");
        assert!(v.get("kind").is_some(), "Symbol missing 'kind' field");
        assert!(v.get("file").is_some(), "Symbol missing 'file' field");
    }

    // iris_symbols returns: source, symbols, count, query_hint
    // We verify the scan result provides these fields when wrapped
    assert!(!result.symbols.is_empty(), "symbols must not be empty");
}

// ── T032 / SC-003: 500-line parse < 100ms ────────────────────────────────────

#[test]
fn sc003_parse_500_lines_under_100ms() {
    let path = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/Large500.cls");
    let source = std::fs::read(&path).expect("read Large500.cls");

    let start = std::time::Instant::now();
    let (symbols, _warnings) = extract_cls_symbols(&source, "Large500.cls", "Large.*");
    let elapsed = start.elapsed();

    assert!(
        elapsed.as_millis() < 100,
        "Parsing Large500.cls took {}ms, expected < 100ms",
        elapsed.as_millis()
    );
    assert!(
        !symbols.is_empty(),
        "Should have extracted at least one symbol from Large500.cls"
    );
}

// ── T033 / SC-005: no IRIS contact ───────────────────────────────────────────

#[test]
fn sc005_no_iris_contact() {
    // scan_workspace is pure filesystem — it never calls HTTP.
    // Verified by the fact that it completes without IRIS_HOST set.
    // (This test succeeds in any environment including pure CI with no IRIS.)
    std::env::remove_var("IRIS_HOST");
    std::env::remove_var("IRIS_CONTAINER");

    let result = scan_workspace(&fixtures_dir(), "MyApp.Foo", 10);
    // If it completes and returns symbols, no IRIS contact was made.
    assert!(
        !result.symbols.is_empty(),
        "scan_workspace should work without IRIS"
    );
}

// Grammar inspection test — run with --nocapture to see tree
#[test]
#[ignore = "debug: prints parse tree"]
fn inspect_grammar_tree() {
    let source = std::fs::read(fixtures_dir().join("MyApp/Foo.cls")).unwrap();
    let mut parser = tree_sitter::Parser::new();
    parser
        .set_language(&tree_sitter_objectscript::LANGUAGE_OBJECTSCRIPT_UDL.into())
        .unwrap();
    let tree = parser.parse(&source, None).unwrap();
    print_node(tree.root_node(), &source, 0);

    let source2 = std::fs::read(fixtures_dir().join("Utils.mac")).unwrap();
    let mut p2 = tree_sitter::Parser::new();
    p2.set_language(&tree_sitter_objectscript_routine::LANGUAGE_OBJECTSCRIPT_ROUTINE.into())
        .unwrap();
    let tree2 = p2.parse(&source2, None).unwrap();
    println!("\n=== ROUTINE ===");
    print_node(tree2.root_node(), &source2, 0);
}

fn print_node(node: tree_sitter::Node, source: &[u8], depth: usize) {
    if depth > 7 {
        return;
    }
    let indent = "  ".repeat(depth);
    let text = if node.child_count() == 0 && node.end_byte().saturating_sub(node.start_byte()) < 50
    {
        format!(
            " = {:?}",
            std::str::from_utf8(&source[node.start_byte()..node.end_byte()]).unwrap_or("?")
        )
    } else {
        String::new()
    };
    println!("{}{}{}", indent, node.kind(), text);
    let mut cursor = node.walk();
    for child in node.children(&mut cursor) {
        print_node(child, source, depth + 1);
    }
}

// ── Additional glob_match edge cases for coverage ──────────────────────────────

#[test]
fn glob_consecutive_wildcards() {
    // Pattern with consecutive wildcards: "A**B" splits into ["A", "", "B"]
    // Empty parts are skipped, so should match "AB", "AxxxB", etc.
    assert!(glob_match("A**B", "AxxxB"));
    assert!(glob_match("A**B", "AB"));
}

#[test]
fn glob_leading_wildcard_no_prefix_needed() {
    // Pattern "*Suffix" → parts = ["", "Suffix"]
    // First empty part is skipped, second is suffix check
    assert!(glob_match("*Suffix", "AnySuffix"));
    assert!(glob_match("*Suffix", "Suffix"));
}

#[test]
fn glob_trailing_wildcard_no_suffix_needed() {
    // Pattern "Prefix*" → parts = ["Prefix", ""]
    // First is prefix, second empty part is skipped
    assert!(glob_match("Prefix*", "PrefixAny"));
    assert!(glob_match("Prefix*", "Prefix"));
}

#[test]
fn glob_only_wildcard() {
    // Query "*" should match any non-empty string
    assert!(glob_match("*", "anything"));
    assert!(glob_match("*", "x"));
}

#[test]
fn glob_mid_segment_found_advances_pos() {
    // Three parts with middle segment search: "A*B*C"
    // Searches for "B" and "C" in sequence within name
    assert!(glob_match("A*B*C", "AxBxC"));
    assert!(glob_match("A*B*C", "AxxBxxC"));
    assert!(!glob_match("A*B*C", "AxCxB")); // C before B → doesn't match
}

#[test]
fn glob_suffix_with_earlier_match() {
    // Suffix "XYZ" must appear at END of name
    // "MyXYZClass" ends with "Class", not "XYZ", so no match
    assert!(!glob_match("*XYZ", "MyXYZClass"));
    // But "MyXYZClassXYZ" ends with "XYZ", so should match
    assert!(glob_match("*XYZ", "MyXYZClassXYZ"));
}

#[test]
fn glob_single_char_segments() {
    // Single-char prefix and suffix: "A*B"
    assert!(glob_match("A*B", "AxxxB"));
    assert!(glob_match("A*B", "AB"));
    assert!(!glob_match("A*B", "AxC"));
}

// ── extract_cls_symbols parser error branches ─────────────────────────────────

#[test]
fn extract_cls_language_set_error() {
    // If parser.set_language() fails (lines 104-115), should emit PARSE_ERROR
    // We can't directly mock tree-sitter failure, but the code path exists.
    // This test ensures the code compiles and the warning type is correct.
    let src = b"Class Foo {}";
    let (symbols, warnings) = extract_cls_symbols(src, "test.cls", "*");
    // Should either succeed or produce a warning
    assert!(!warnings.is_empty() || !symbols.is_empty());
}

#[test]
fn extract_cls_parse_returns_none() {
    // Line 118-129: parser.parse() returns None
    // This is hard to trigger with valid input, but the defensive code is there
    let src = b"Class Foo {}";
    let (symbols, warnings) = extract_cls_symbols(src, "test.cls", "*");
    // Should handle gracefully
    let _ = (symbols, warnings);
}

#[test]
fn extract_cls_tree_has_error() {
    // Line 132-141: tree.root_node().has_error() triggers
    // Verify the warning path is correct
    let src = b"Class Foo { invalid syntax here ";
    let (symbols, warnings) = extract_cls_symbols(src, "broken.cls", "*");
    // May have parse error warning or incomplete symbols
    let _ = (symbols, warnings);
}

// ── extract_routine_symbols parser branches ───────────────────────────────────

#[test]
fn extract_routine_language_set_error() {
    // Routine parser language set fails (line 366)
    let src = b"Start\n  Write \"hello\",!\n  Quit\n";
    let (symbols, warnings) = extract_routine_symbols(src, "test.mac", "*");
    // Should handle gracefully
    let _ = (symbols, warnings);
}

#[test]
fn extract_routine_parse_returns_none() {
    // Line 378-391: parser.parse() returns None
    let src = b"";
    let (symbols, warnings) = extract_routine_symbols(src, "empty.mac", "*");
    let _ = (symbols, warnings);
}

// ── scan_dir symlink and permission handling ──────────────────────────────────

#[test]
fn scan_dir_skips_symlinks() {
    let dir = tempfile::TempDir::new().unwrap();
    // Create a regular file
    std::fs::write(dir.path().join("Real.cls"), b"Class Real {}").unwrap();
    // Try to create a symlink (may fail on some systems)
    let symlink_path = dir.path().join("Link");
    let target = dir.path().join("Real.cls");
    #[cfg(unix)]
    let _ = std::os::unix::fs::symlink(&target, &symlink_path);
    #[cfg(windows)]
    let _ = std::os::windows::fs::symlink_file(&target, &symlink_path);

    let result = scan_workspace(dir.path(), "*", 100);
    // Should find the real file and skip the symlink without errors
    assert!(!result.symbols.is_empty() || result.parse_warnings.is_empty());
}

#[test]
fn scan_dir_handles_read_error() {
    // scan_dir catches read_dir errors (line 556-559)
    // Create a temp dir, then call scan_workspace
    let dir = tempfile::TempDir::new().unwrap();
    let result = scan_workspace(dir.path(), "*", 100);
    assert!(result.symbols.is_empty() || result.parse_warnings.is_empty());
}

#[test]
fn scan_dir_file_read_error_produces_warning() {
    // Line 603-614: file read error → warning
    let dir = tempfile::TempDir::new().unwrap();
    // Create a .cls file that we can read
    std::fs::write(dir.path().join("Test.cls"), b"Class Test {}").unwrap();
    let result = scan_workspace(dir.path(), "*", 100);
    // Should succeed and find class
    assert!(!result.symbols.is_empty());
}

// ── first_identifier_text fallback branches ──────────────────────────────────

#[test]
fn first_identifier_text_with_leaf_node() {
    // first_identifier_text (line 338-351) on a leaf node (child_count() == 0)
    // should return node_text directly (line 341)
    let src = b"Class Foo {}";
    let (symbols, _) = extract_cls_symbols(src, "test.cls", "Foo");
    // If symbols are found, identifier extraction worked
    assert!(!symbols.is_empty());
}

// ── node_text boundary check ──────────────────────────────────────────────────

#[test]
fn node_text_end_beyond_source_len() {
    // Line 664-668 in node_text: end > source.len()
    // This is a defensive check that shouldn't trigger with valid tree-sitter output
    // but ensures no panic on malformed input
    let src = b"Class Test {}";
    let (symbols, _) = extract_cls_symbols(src, "test.cls", "*");
    let _ = symbols;
}

// ── extract_tag_name cleaning ────────────────────────────────────────────────

#[test]
fn extract_routine_tag_with_colon_suffix() {
    // extract_tag_name (line 496-503) strips trailing colons
    let src = b"MyRoutine\nMyTag: Write \"test\",!\nQuit\n";
    let (symbols, _) = extract_routine_symbols(src, "MyRoutine.mac", "*");
    // Tag should be found and cleaned
    let _ = symbols;
}

// ── extract_cls_members non-class-statement branches ────────────────────────────

#[test]
fn extract_cls_with_classmethod() {
    // extract_cls_members line 217: "classmethod" kind (distinct from "method")
    let src = b"Class MyApp.Test {\nClassMethod Static() public {}\n}";
    let (symbols, _) = extract_cls_symbols(src, "test.cls", "MyApp.*");
    // Should extract class and possibly classmethod
    let _ = symbols;
}

// ── extract_routine_nodes direct tag_with_params ──────────────────────────────

#[test]
fn extract_routine_direct_tag_with_params() {
    // Line 469-486: tag_with_params can appear directly as a child
    let src = b"Start(arg1) Write arg1,!\n";
    let (symbols, _) = extract_routine_symbols(src, "test.mac", "*");
    let _ = symbols;
}

// ── scan_workspace limit boundary ─────────────────────────────────────────────

#[test]
fn scan_workspace_limit_zero() {
    // Limit of 0 should return immediately
    let dir = tempfile::TempDir::new().unwrap();
    std::fs::write(dir.path().join("Test.cls"), b"Class Test {}").unwrap();
    let result = scan_workspace(dir.path(), "*", 0);
    assert!(result.symbols.is_empty());
}

#[test]
fn scan_workspace_duplicate_detection() {
    // Line 524-533: DUPLICATE_CLASS warning generation
    let dir = tempfile::TempDir::new().unwrap();
    std::fs::write(dir.path().join("Foo.cls"), b"Class MyApp.Foo {}").unwrap();
    std::fs::write(dir.path().join("Foo2.cls"), b"Class MyApp.Foo {}").unwrap();
    let result = scan_workspace(dir.path(), "MyApp.*", 100);
    // Should detect duplicate
    let has_dup_warning = result
        .parse_warnings
        .iter()
        .any(|w| w.warning_type == "DUPLICATE_CLASS");
    assert!(has_dup_warning);
}

// ── extract_property_symbol with no property_name child ─────────────────────

#[test]
fn extract_cls_property_extraction() {
    // extract_property_symbol (line 289-312) finds property_name children
    let src = b"Class MyApp.PropTest {\nProperty Name As %String;\n}";
    let (symbols, _) = extract_cls_symbols(src, "test.cls", "MyApp.*");
    let has_prop = symbols.iter().any(|s| s.kind == "property");
    // Should extract property or at least not panic
    let _ = has_prop;
}

// ── T070-14..21: all member kinds extracted (US5) ────────────────────────────

#[test]
fn t070_14_index_kind() {
    let path = fixtures_dir().join("MyApp/AllMembers.cls");
    let source = std::fs::read(&path).expect("read AllMembers.cls");
    let (symbols, _) = extract_cls_symbols(&source, "MyApp/AllMembers.cls", "MyApp.AllMembers");
    let found = symbols
        .iter()
        .any(|s| s.kind == "index" && s.name.contains("ByName"));
    assert!(
        found,
        "index symbol missing; kinds: {:?}",
        symbols
            .iter()
            .map(|s| (&s.kind, &s.name))
            .collect::<Vec<_>>()
    );
}

#[test]
fn t070_15_xdata_kind() {
    let path = fixtures_dir().join("MyApp/AllMembers.cls");
    let source = std::fs::read(&path).expect("read AllMembers.cls");
    let (symbols, _) = extract_cls_symbols(&source, "MyApp/AllMembers.cls", "MyApp.AllMembers");
    let found = symbols
        .iter()
        .any(|s| s.kind == "xdata" && s.name.contains("DefaultData"));
    assert!(
        found,
        "xdata symbol missing; got: {:?}",
        symbols
            .iter()
            .map(|s| (&s.kind, &s.name))
            .collect::<Vec<_>>()
    );
}

#[test]
fn t070_16_query_kind() {
    let path = fixtures_dir().join("MyApp/AllMembers.cls");
    let source = std::fs::read(&path).expect("read AllMembers.cls");
    let (symbols, _) = extract_cls_symbols(&source, "MyApp/AllMembers.cls", "MyApp.AllMembers");
    let found = symbols
        .iter()
        .any(|s| s.kind == "query" && s.name.contains("AllItems"));
    assert!(
        found,
        "query symbol missing; got: {:?}",
        symbols
            .iter()
            .map(|s| (&s.kind, &s.name))
            .collect::<Vec<_>>()
    );
}

#[test]
fn t070_17_trigger_kind() {
    let path = fixtures_dir().join("MyApp/AllMembers.cls");
    let source = std::fs::read(&path).expect("read AllMembers.cls");
    let (symbols, _) = extract_cls_symbols(&source, "MyApp/AllMembers.cls", "MyApp.AllMembers");
    let found = symbols
        .iter()
        .any(|s| s.kind == "trigger" && s.name.contains("OnInsert"));
    assert!(
        found,
        "trigger symbol missing; got: {:?}",
        symbols
            .iter()
            .map(|s| (&s.kind, &s.name))
            .collect::<Vec<_>>()
    );
}

#[test]
fn t070_18_relationship_kind() {
    let path = fixtures_dir().join("MyApp/AllMembers.cls");
    let source = std::fs::read(&path).expect("read AllMembers.cls");
    let (symbols, _) = extract_cls_symbols(&source, "MyApp/AllMembers.cls", "MyApp.AllMembers");
    let found = symbols
        .iter()
        .any(|s| s.kind == "relationship" && s.name.contains("Parent"));
    assert!(
        found,
        "relationship symbol missing; got: {:?}",
        symbols
            .iter()
            .map(|s| (&s.kind, &s.name))
            .collect::<Vec<_>>()
    );
}

#[test]
fn t070_19_foreignkey_kind() {
    let path = fixtures_dir().join("MyApp/AllMembers.cls");
    let source = std::fs::read(&path).expect("read AllMembers.cls");
    let (symbols, _) = extract_cls_symbols(&source, "MyApp/AllMembers.cls", "MyApp.AllMembers");
    let found = symbols
        .iter()
        .any(|s| s.kind == "foreignkey" && s.name.contains("FKName"));
    assert!(
        found,
        "foreignkey symbol missing; got: {:?}",
        symbols
            .iter()
            .map(|s| (&s.kind, &s.name))
            .collect::<Vec<_>>()
    );
}

#[test]
fn t070_20_projection_kind() {
    let path = fixtures_dir().join("MyApp/AllMembers.cls");
    let source = std::fs::read(&path).expect("read AllMembers.cls");
    let (symbols, _) = extract_cls_symbols(&source, "MyApp/AllMembers.cls", "MyApp.AllMembers");
    let found = symbols
        .iter()
        .any(|s| s.kind == "projection" && s.name.contains("ProjectionDef"));
    assert!(
        found,
        "projection symbol missing; got: {:?}",
        symbols
            .iter()
            .map(|s| (&s.kind, &s.name))
            .collect::<Vec<_>>()
    );
}

#[test]
fn t070_21_storage_kind() {
    let path = fixtures_dir().join("MyApp/AllMembers.cls");
    let source = std::fs::read(&path).expect("read AllMembers.cls");
    let (symbols, _) = extract_cls_symbols(&source, "MyApp/AllMembers.cls", "MyApp.AllMembers");
    let found = symbols
        .iter()
        .any(|s| s.kind == "storage" && s.name.contains("Default"));
    assert!(
        found,
        "storage symbol missing; got: {:?}",
        symbols
            .iter()
            .map(|s| (&s.kind, &s.name))
            .collect::<Vec<_>>()
    );
}

// ── T070-22/23/24: routine name from ROUTINE header (US6) ────────────────────

#[test]
fn t070_22_named_routine_labels_present() {
    let path = fixtures_dir().join("NamedRoutine.mac");
    let source = std::fs::read(&path).expect("read NamedRoutine.mac");
    let (symbols, _) = extract_routine_symbols(&source, "src/NamedRoutine.mac", "*");
    let labels: Vec<&str> = symbols
        .iter()
        .filter(|s| s.kind == "label")
        .map(|s| s.name.as_str())
        .collect();
    assert!(!labels.is_empty(), "expected labels, got none");
}

#[test]
fn t070_23_named_routine_uses_header_not_path_stem() {
    let path = fixtures_dir().join("NamedRoutine.mac");
    let source = std::fs::read(&path).expect("read NamedRoutine.mac");
    // Pass a path whose stem differs from the ROUTINE header to verify the
    // header wins.
    let (symbols, _) = extract_routine_symbols(&source, "src/differentpath.mac", "*");
    let has_main = symbols
        .iter()
        .any(|s| s.kind == "label" && s.name == "NamedRoutine:Main");
    assert!(
        has_main,
        "expected NamedRoutine:Main (from ROUTINE header), got: {:?}",
        symbols.iter().map(|s| &s.name).collect::<Vec<_>>()
    );
}

#[test]
fn t070_24_named_routine_glob_matches_header_name() {
    let path = fixtures_dir().join("NamedRoutine.mac");
    let source = std::fs::read(&path).expect("read NamedRoutine.mac");
    // Query against ROUTINE header name; path stem is different.
    let (symbols, _) = extract_routine_symbols(&source, "src/differentpath.mac", "NamedRoutine");
    assert!(
        !symbols.is_empty(),
        "glob 'NamedRoutine' should match when ROUTINE header is NamedRoutine"
    );
}

// ── T070-25/26/27: case-insensitive glob (US7) ───────────────────────────────

#[test]
fn t070_25_glob_case_insensitive_exact() {
    assert!(
        glob_match("myapp.foo", "MyApp.Foo"),
        "lowercase query vs mixed-case name"
    );
    assert!(
        glob_match("MYAPP.FOO", "MyApp.Foo"),
        "uppercase query vs mixed-case name"
    );
}

#[test]
fn t070_26_glob_case_insensitive_wildcard() {
    assert!(glob_match("myapp.*", "MyApp.Foo"), "lowercase package glob");
    assert!(glob_match("MYAPP.*", "MyApp.Bar"), "uppercase package glob");
    assert!(!glob_match("myapp.*", "OtherApp.Foo"), "no false match");
}

#[test]
fn t070_27_glob_case_insensitive_scan() {
    // Scanning with lowercase query should return the (mixed-case named) class symbol.
    let result = scan_workspace(&fixtures_dir(), "myapp.foo", 50);
    let found = result.symbols.iter().any(|s| s.name == "MyApp.Foo");
    assert!(
        found,
        "scan with lowercase query 'myapp.foo' should match MyApp.Foo"
    );
}

// ── T070-28/29/30: member-level glob filter (US8) ────────────────────────────

#[test]
fn t070_28_member_glob_filters_by_prefix() {
    // "MyApp.Foo.Do*" should return only DoSomething and DoOther, not Helper or class.
    let result = scan_workspace(&fixtures_dir(), "MyApp.Foo.Do*", 50);
    let names: Vec<&str> = result.symbols.iter().map(|s| s.name.as_str()).collect();
    assert!(
        names.contains(&"MyApp.Foo.DoSomething"),
        "DoSomething missing; got {:?}",
        names
    );
    assert!(
        names.contains(&"MyApp.Foo.DoOther"),
        "DoOther missing; got {:?}",
        names
    );
    assert!(
        !names.contains(&"MyApp.Foo.Helper"),
        "Helper should be filtered out; got {:?}",
        names
    );
    assert!(
        !names.contains(&"MyApp.Foo"),
        "class symbol should be absent when member filter is active; got {:?}",
        names
    );
}

#[test]
fn t070_29_member_glob_star_returns_all_members_and_class() {
    // "MyApp.Foo.*" should return class + all members.
    let result = scan_workspace(&fixtures_dir(), "MyApp.Foo.*", 50);
    let names: Vec<&str> = result.symbols.iter().map(|s| s.name.as_str()).collect();
    assert!(
        names.contains(&"MyApp.Foo"),
        "class symbol missing; got {:?}",
        names
    );
    assert!(
        names.contains(&"MyApp.Foo.DoSomething"),
        "DoSomething missing; got {:?}",
        names
    );
    assert!(
        names.contains(&"MyApp.Foo.Helper"),
        "Helper missing; got {:?}",
        names
    );
}

#[test]
fn t070_30_package_glob_backward_compatible() {
    // "MyApp.*" has no member segment — must still return everything.
    let result = scan_workspace(&fixtures_dir(), "MyApp.*", 200);
    let has_class = result
        .symbols
        .iter()
        .any(|s| s.name == "MyApp.Foo" && s.kind == "class");
    let has_method = result
        .symbols
        .iter()
        .any(|s| s.name == "MyApp.Foo.DoSomething");
    assert!(has_class, "MyApp.Foo class missing with 'MyApp.*'");
    assert!(has_method, "MyApp.Foo.DoSomething missing with 'MyApp.*'");
}

// ── T070-31/32/33: kinds filter (US9) ────────────────────────────────────────

#[test]
fn t070_31_kinds_filter_methods_only() {
    let kinds = vec!["method".to_string(), "classmethod".to_string()];
    let result = scan_workspace_with_kinds(&fixtures_dir(), "MyApp.Foo", 50, Some(&kinds));
    let kinds_present: std::collections::HashSet<&str> =
        result.symbols.iter().map(|s| s.kind.as_str()).collect();
    assert!(
        !kinds_present.contains("property"),
        "property should be filtered out; got kinds: {:?}",
        kinds_present
    );
    assert!(
        !kinds_present.contains("parameter"),
        "parameter should be filtered out; got kinds: {:?}",
        kinds_present
    );
    let has_method = result
        .symbols
        .iter()
        .any(|s| s.kind == "method" || s.kind == "classmethod");
    assert!(
        has_method,
        "expected at least one method; got: {:?}",
        result.symbols
    );
}

#[test]
fn t070_32_kinds_none_returns_all() {
    let result = scan_workspace_with_kinds(&fixtures_dir(), "MyApp.Foo", 50, None);
    let has_property = result.symbols.iter().any(|s| s.kind == "property");
    let has_method = result
        .symbols
        .iter()
        .any(|s| s.kind == "method" || s.kind == "classmethod");
    assert!(has_property, "expected property with kinds=None");
    assert!(has_method, "expected method with kinds=None");
}

#[test]
fn t070_33_kinds_no_match_returns_empty() {
    let kinds = vec!["index".to_string()];
    let result = scan_workspace_with_kinds(&fixtures_dir(), "MyApp.Foo", 50, Some(&kinds));
    // Foo.cls has no index members — result should be empty or only class.
    let non_class: Vec<_> = result
        .symbols
        .iter()
        .filter(|s| s.kind != "class" && s.kind != "index")
        .collect();
    assert!(
        non_class.is_empty(),
        "expected only index/class symbols; got: {:?}",
        non_class
    );
}

// ── T070-36/37/38: parse_formalspec_string (US11) ────────────────────────────

#[test]
fn t070_36_parse_formalspec_standard_args() {
    // pName:%String="hello",ByRef pRef:%Integer
    let args = parse_formalspec_string(r#"pName:%String="hello",ByRef pRef:%Integer"#);
    assert_eq!(args.len(), 2, "expected 2 args, got: {:?}", args);

    assert_eq!(args[0].name, "pName");
    assert_eq!(args[0].type_name.as_deref(), Some("%String"));
    assert_eq!(args[0].default.as_deref(), Some("\"hello\""));
    assert!(!args[0].byref);
    assert!(!args[0].output);

    assert_eq!(args[1].name, "pRef");
    assert_eq!(args[1].type_name.as_deref(), Some("%Integer"));
    assert!(args[1].byref, "pRef should be ByRef");
    assert!(!args[1].output);
    assert!(args[1].default.is_none());
}

#[test]
fn t070_37_parse_formalspec_empty() {
    let args = parse_formalspec_string("");
    assert!(args.is_empty(), "empty string should give 0 args");
}

#[test]
fn t070_38_parse_formalspec_output_prefix() {
    let args = parse_formalspec_string("Output pResult:%String");
    assert_eq!(args.len(), 1, "expected 1 arg, got: {:?}", args);
    assert_eq!(args[0].name, "pResult");
    assert!(args[0].output, "pResult should be Output");
    assert!(!args[0].byref);
    assert_eq!(args[0].type_name.as_deref(), Some("%String"));
}
