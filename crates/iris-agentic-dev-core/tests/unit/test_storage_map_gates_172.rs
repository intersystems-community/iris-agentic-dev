//! Tests for storage map gate integration (spec 172, Phase 5, US8: T052-T059).
//!
//! Run: `RUST_TEST_THREADS=12 cargo test --features testing --test unit test_storage_map_gates_172`

use iris_agentic_dev_core::tools::storage_map::code::{code_elements, CODE_TAGS};
use iris_agentic_dev_core::tools::storage_map::tree::{parse_body, split_class, Node};
use std::fs;
use std::path::PathBuf;

fn fixture_path(name: &str) -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures/storage172/classes")
        .join(format!("{name}.cls"))
}

fn load_fixture(name: &str) -> String {
    fs::read_to_string(fixture_path(name))
        .unwrap_or_else(|_| panic!("fixture {name}.cls not found"))
}

fn parse_fixture(name: &str) -> Vec<Node> {
    let text = load_fixture(name);
    let spans = split_class(&text).expect("split_class failed");
    let span = spans.first().expect("no storage block");
    parse_body(&text[span.body_start..span.body_end]).expect("parse_body failed")
}

// ─────────────────────────────────────────────────────────────────────────────
// T052: code_elements extracts code from storage maps
// ─────────────────────────────────────────────────────────────────────────────

#[test]
fn test_code_tags_list() {
    // CODE_TAGS must contain the tags from research R6.
    let expected = &[
        "Expression",
        "DataAccess",
        "NextCode",
        "StopExpression",
        "Code", // AccessVar Code
        "RetrievalCode",
        "RowReference",
        "Condition",
        "Global", // when it's an extended reference, treated as code
        "Invalidcondition",
    ];
    for tag in expected {
        assert!(
            CODE_TAGS.contains(tag),
            "CODE_TAGS missing expected tag: {tag}"
        );
    }
}

#[test]
fn test_shelf_code_elements() {
    // Shelf.cls has NextCode with multi-line code containing '}'.
    let nodes = parse_fixture("Shelf");
    let elements = code_elements(&nodes);

    // Should find the NextCode.
    let next_codes: Vec<_> = elements
        .iter()
        .filter(|(path, _)| path.contains("NextCode"))
        .collect();
    assert!(!next_codes.is_empty(), "Shelf should have NextCode");

    // The NextCode value should contain the loop with '}'.
    let (_path, code) = &next_codes[0];
    assert!(code.contains("$o("), "NextCode should have $order");
    assert!(code.contains("}"), "NextCode should have closing brace");
}

#[test]
fn test_branch_code_elements() {
    // Branch.cls fixture should have a computed field if it exists.
    // If Branch doesn't have RetrievalCode, this test passes vacuously.
    let nodes = parse_fixture("Branch");
    let elements = code_elements(&nodes);

    let retrieval_codes: Vec<_> = elements
        .iter()
        .filter(|(path, _)| path.contains("RetrievalCode"))
        .collect();

    // This passes if there are no retrieval codes, or if they have raw characters
    if !retrieval_codes.is_empty() {
        let (_path, code) = &retrieval_codes[0];
        // The code should have raw characters (not entities).
        assert!(
            code.contains('<') || code.contains('&') || code.contains('{'),
            "RetrievalCode should have raw special characters"
        );
    }
}

#[test]
fn test_unknown_tag_treated_as_code() {
    // An element whose tag is not in LITERAL_TAGS should be yielded as code.
    let nodes = vec![Node {
        tag: "UnknownTag".to_string(),
        name: None,
        value: Some("some code".to_string()),
        children: None,
    }];
    let elements = code_elements(&nodes);
    assert_eq!(elements.len(), 1, "unknown tag should be treated as code");
    assert_eq!(elements[0].1, "some code");
}

#[test]
fn test_literal_tags_not_extracted() {
    // LITERAL_TAGS should not appear in code_elements output.
    let nodes = vec![
        Node {
            tag: "Type".to_string(),
            name: None,
            value: Some("data".to_string()),
            children: None,
        },
        Node {
            tag: "Delimiter".to_string(),
            name: None,
            value: Some("^".to_string()),
            children: None,
        },
        Node {
            tag: "Piece".to_string(),
            name: None,
            value: Some("1".to_string()),
            children: None,
        },
    ];
    let elements = code_elements(&nodes);
    assert!(elements.is_empty(), "literal tags should not be extracted");
}

// ─────────────────────────────────────────────────────────────────────────────
// T053: check_map_code runs code edit gate
// ─────────────────────────────────────────────────────────────────────────────

#[test]
fn test_code_storage_globals_refused() {
    use iris_agentic_dev_core::tools::storage_map::code::check_map_code;

    // NextCode that names a code-storage global should be refused.
    let nodes = vec![Node {
        tag: "NextCode".to_string(),
        name: None,
        value: Some("s {L1}=$o(^ROUTINE({L1}))".to_string()),
        children: None,
    }];

    let result = check_map_code(&nodes, "test-server");
    assert!(result.is_some(), "^ROUTINE should be refused");
    let err = result.unwrap();
    assert_eq!(err["error_code"], "CODE_EDIT_BLOCKED");
}

#[test]
fn test_system_compile_refused() {
    use iris_agentic_dev_core::tools::storage_map::code::check_map_code;

    // Code that uses $system.OBJ.Compile should be refused.
    let nodes = vec![Node {
        tag: "RetrievalCode".to_string(),
        name: None,
        value: Some("d $system.OBJ.Compile(cls)".to_string()),
        children: None,
    }];

    let result = check_map_code(&nodes, "test-server");
    assert!(result.is_some(), "$system.OBJ.Compile should be refused");
    let err = result.unwrap();
    assert_eq!(err["error_code"], "CODE_EDIT_BLOCKED");
}

#[test]
fn test_shelf_next_code_passes() {
    use iris_agentic_dev_core::tools::storage_map::code::check_map_code;

    // Shelf's NextCode is benign (walks a non-code global).
    let nodes = parse_fixture("Shelf");
    let result = check_map_code(&nodes, "test-server");
    assert!(result.is_none(), "Shelf's NextCode should pass");
}

// ─────────────────────────────────────────────────────────────────────────────
// T054: dynamic_globals and code_changed
// ─────────────────────────────────────────────────────────────────────────────

#[test]
fn test_dynamic_globals_detected() {
    use iris_agentic_dev_core::tools::storage_map::code::dynamic_globals;

    // AccessType Global
    let nodes1 = vec![Node {
        tag: "AccessType".to_string(),
        name: None,
        value: Some("Global".to_string()),
        children: None,
    }];
    let dyn1 = dynamic_globals(&nodes1);
    assert_eq!(dyn1.len(), 1);
    assert_eq!(dyn1[0], "AccessType: Global");

    // @ indirection in Global
    let nodes2 = vec![Node {
        tag: "Global".to_string(),
        name: None,
        value: Some("^@globalName".to_string()),
        children: None,
    }];
    let dyn2 = dynamic_globals(&nodes2);
    assert_eq!(dyn2.len(), 1);
    assert!(dyn2[0].contains("@"));

    // $Name in DataAccess
    let nodes3 = vec![Node {
        tag: "DataAccess".to_string(),
        name: None,
        value: Some("$Name(^Global(1,x))".to_string()),
        children: None,
    }];
    let dyn3 = dynamic_globals(&nodes3);
    assert_eq!(dyn3.len(), 1);
    assert!(dyn3[0].contains("$Name") || dyn3[0].contains("$NAME"));
}

#[test]
fn test_plain_map_no_dynamic() {
    use iris_agentic_dev_core::tools::storage_map::code::dynamic_globals;

    // Plain map with no dynamic references.
    let nodes = vec![Node {
        tag: "Global".to_string(),
        name: None,
        value: Some("^IadSm172Member".to_string()),
        children: None,
    }];
    let dyn_refs = dynamic_globals(&nodes);
    assert!(dyn_refs.is_empty(), "plain global should not be dynamic");
}

#[test]
fn test_code_changed_detects_nextcode_change() {
    use iris_agentic_dev_core::tools::storage_map::code::code_changed;

    let before = vec![Node {
        tag: "NextCode".to_string(),
        name: None,
        value: Some("s {L1}=$o(^G({L1}))".to_string()),
        children: None,
    }];

    let after = vec![Node {
        tag: "NextCode".to_string(),
        name: None,
        value: Some("s {L1}=$o(^G({L1}),1)".to_string()),
        children: None,
    }];

    assert!(
        code_changed(&before, &after),
        "NextCode change should be detected"
    );
}

#[test]
fn test_code_changed_false_for_piece_change() {
    use iris_agentic_dev_core::tools::storage_map::code::code_changed;

    let before = vec![Node {
        tag: "Piece".to_string(),
        name: None,
        value: Some("1".to_string()),
        children: None,
    }];

    let after = vec![Node {
        tag: "Piece".to_string(),
        name: None,
        value: Some("2".to_string()),
        children: None,
    }];

    assert!(
        !code_changed(&before, &after),
        "Piece change is not code change"
    );
}

// ─────────────────────────────────────────────────────────────────────────────
// T055: check_map_globals, through gate_tree (the one entry point a handler calls)
// ─────────────────────────────────────────────────────────────────────────────

use iris_agentic_dev_core::policy::effective::EffectivePolicy;
use iris_agentic_dev_core::tools::storage_map::call_shape;
use iris_agentic_dev_core::tools::storage_map::code::gate_tree;
use iris_agentic_dev_core::tools::storage_map::globals::global_tokens;
use serde_json::{json, Value};

fn leaf(tag: &str, value: &str) -> Node {
    Node {
        tag: tag.to_string(),
        name: None,
        value: Some(value.to_string()),
        children: None,
    }
}

/// A one-map tree whose map carries `extra` beside a harmless global.
fn map_with(extra: Vec<Node>) -> Vec<Node> {
    let mut children = vec![leaf("Global", "^IadSm172Member"), leaf("Type", "data")];
    children.extend(extra);
    vec![Node {
        tag: "SQLMap".to_string(),
        name: Some("Master".to_string()),
        value: None,
        children: Some(children),
    }]
}

/// The gate's answer for `nodes` under `action` (read through the real `call_shape`).
fn gate(nodes: &[Node], params: Value, policy: &EffectivePolicy) -> Option<Value> {
    gate_tree(nodes, &call_shape(&params), policy, "s1")
}

fn edit() -> Value {
    json!({"action": "edit"})
}

fn code_of(r: &Option<Value>) -> &str {
    r.as_ref()
        .and_then(|v| v["error_code"].as_str())
        .unwrap_or("<passed>")
}

#[test]
fn code_storage_globals_are_refused_in_every_spelling_and_place() {
    let p = EffectivePolicy::default();
    for (tag, value) in [
        ("Global", "^ROUTINE"),
        ("Global", "ROUTINE"),
        ("Global", "^routine"),
        ("Global", "^rOBJ"),
        ("global", "^oddDEF"),
        ("IdLocation", "^oddDEF"),
        ("DataLocation", "oddDEF(\"x\")"),
        ("IndexLocation", "^%Dictionary.ClassDefinitionD"),
        ("SqlIdExpression", "$i(^oddDEF)"),
        ("SqlIdExpression", "$INCREMENT(^ROUTINE(\"c\"))"),
    ] {
        let r = gate(&map_with(vec![leaf(tag, value)]), edit(), &p);
        assert_eq!(code_of(&r), "CODE_EDIT_BLOCKED", "{tag} {value}: {r:?}");
    }
}

#[test]
fn blocklisted_globals_are_refused_with_both_spellings() {
    let mut p = EffectivePolicy::default();
    p.global_blocklist.value = vec!["^IadSecret*".to_string()];
    for value in ["^IadSecretX", "IadSecretX", "^iadsecretx(1)"] {
        let r = gate(&map_with(vec![leaf("DataLocation", value)]), edit(), &p);
        assert_eq!(code_of(&r), "SYSTEM_BLOCKLIST", "{value}: {r:?}");
        assert_eq!(r.unwrap()["custom_pattern"], true, "{value}");
    }
}

#[test]
fn phi_named_globals_need_acknowledge_phi() {
    let p = EffectivePolicy::default();
    let nodes = map_with(vec![leaf("Global", "^PAPMI")]);
    assert_eq!(code_of(&gate(&nodes, edit(), &p)), "PHI_GATE_BLOCKED");
    assert_eq!(
        code_of(&gate(
            &nodes,
            json!({"action": "edit", "acknowledgePhi": "true"}),
            &p
        )),
        "PHI_GATE_BLOCKED",
        "a string is not true"
    );
    assert!(gate(
        &nodes,
        json!({"action": "edit", "acknowledgePhi": true}),
        &p
    )
    .is_none());
}

#[test]
fn extended_references_are_refused_for_writes_and_shown_for_reads() {
    let p = EffectivePolicy::default();
    for value in ["^[\"%SYS\"]IadX", "^|\"USER\"|IadX", "^|\"^^/db/\"|IadX(1)"] {
        let nodes = map_with(vec![leaf("Global", value)]);
        for action in ["edit", "replace", "check", "compare", "preview", "propose"] {
            let r = gate(&nodes, json!({"action": action}), &p);
            assert_eq!(code_of(&r), "STORAGE_MAP_INVALID", "{action} {value}");
            assert_eq!(r.unwrap()["rule"], "extended_reference", "{action} {value}");
        }
        for action in ["read", "diagram", "uses"] {
            assert!(
                gate(&nodes, json!({"action": action}), &p).is_none(),
                "{action} {value}"
            );
        }
        // An action the reader cannot read is judged as edit, not as the read it resembles.
        let r = gate(&nodes, json!({"action": "Read"}), &p);
        assert_eq!(code_of(&r), "STORAGE_MAP_INVALID", "Read {value}");
    }
}

#[test]
fn an_unreadable_global_value_is_refused_not_passed() {
    let p = EffectivePolicy::default();
    let r = gate(&map_with(vec![leaf("Global", "^|\"|IadX")]), edit(), &p);
    assert_eq!(code_of(&r), "STORAGE_MAP_INVALID");
    assert_eq!(r.unwrap()["rule"], "global_reference");
    let r = gate(&map_with(vec![leaf("IdLocation", "^Iad\"X")]), edit(), &p);
    assert_eq!(code_of(&r), "INVALID_GLOBAL_NAME", "{r:?}");
}

#[test]
fn every_fixture_passes_the_gates_for_edit() {
    let p = EffectivePolicy::default();
    let dir = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/storage172/classes");
    let mut n = 0;
    for entry in fs::read_dir(&dir).expect("fixtures") {
        let name = entry
            .unwrap()
            .path()
            .file_stem()
            .unwrap()
            .to_string_lossy()
            .to_string();
        let r = gate(&parse_fixture(&name), edit(), &p);
        assert!(r.is_none(), "{name}: {r:?}");
        n += 1;
    }
    assert!(n >= 15, "{n} fixtures");
}

#[test]
fn global_tokens_reads_references_and_skips_delimiters() {
    assert_eq!(global_tokens("$P(x,\"^\",2)"), Vec::<String>::new());
    assert_eq!(global_tokens("$i(^Iad.Sm(1))"), vec!["^Iad.Sm"]);
    assert_eq!(
        global_tokens("$i(^|\"NS\"|X)+$G(^[\"NS\"]Y(1))"),
        vec!["^|\"NS\"|X", "^[\"NS\"]Y"]
    );
    assert_eq!(global_tokens("^%SYS"), vec!["^%SYS"]);
    assert_eq!(
        global_tokens("x_^|\"NS"),
        vec!["^|\"NS"],
        "an unterminated prefix is kept, so it is refused"
    );
}

// ─────────────────────────────────────────────────────────────────────────────
// T058: gate_tree runs globals, then code, then dynamic globals
// ─────────────────────────────────────────────────────────────────────────────

#[test]
fn gate_tree_order_is_globals_then_code_then_dynamic() {
    let p = EffectivePolicy::default();
    let bad_code = leaf("NextCode", " d $system.OBJ.Compile(cls)");
    let dynamic = leaf("AccessType", "Global");

    let r = gate(
        &map_with(vec![
            leaf("IdLocation", "^ROUTINE"),
            bad_code.clone(),
            dynamic.clone(),
        ]),
        edit(),
        &p,
    );
    assert_eq!(code_of(&r), "CODE_EDIT_BLOCKED");
    assert_eq!(
        r.unwrap()["tag"],
        "IdLocation",
        "the global check answers first"
    );

    let r = gate(&map_with(vec![bad_code, dynamic.clone()]), edit(), &p);
    assert_eq!(code_of(&r), "CODE_EDIT_BLOCKED");
    assert!(
        r.unwrap().get("tag").is_none(),
        "the code check answers before the dynamic one"
    );

    let r = gate(&map_with(vec![dynamic]), edit(), &p);
    assert_eq!(code_of(&r), "STORAGE_MAP_DYNAMIC_GLOBAL_BLOCKED");
    assert!(
        r.unwrap().get("refusal").is_none(),
        "the dispatcher adds refusal, not the gate"
    );
}

#[test]
fn dynamic_globals_are_found_whatever_the_case() {
    let p = EffectivePolicy::default();
    for (tag, value) in [
        ("AccessType", "Global"),
        ("accesstype", "GLOBAL"),
        ("AccessType", " global "),
        ("DataAccess", "@x"),
        ("rowreference", "$name(^X(1))"),
        ("DataAccess", "$NA(^X)"),
    ] {
        let r = gate(&map_with(vec![leaf(tag, value)]), edit(), &p);
        assert_eq!(
            code_of(&r),
            "STORAGE_MAP_DYNAMIC_GLOBAL_BLOCKED",
            "{tag} {value}: {r:?}"
        );
    }
}

#[test]
fn allow_dynamic_map_globals_lifts_the_dynamic_refusal_and_nothing_else() {
    let mut p = EffectivePolicy::default();
    p.allow_dynamic_map_globals.value = true;
    let dynamic = leaf("DataAccess", "@ref");
    assert!(gate(&map_with(vec![dynamic.clone()]), edit(), &p).is_none());
    // The other gates still hold with the setting on.
    let r = gate(
        &map_with(vec![dynamic, leaf("Global", "^ROUTINE")]),
        edit(),
        &p,
    );
    assert_eq!(code_of(&r), "CODE_EDIT_BLOCKED");
}

/// Review 2026-11-11: a dynamic name in a storage keyword was skipped by the global gates (left to
/// the dynamic check) and never reached the dynamic check, which read only Global, DataAccess and
/// RowReference.
#[test]
fn a_dynamic_name_in_any_global_bearing_tag_is_refused() {
    let p = EffectivePolicy::default();
    for (tag, value) in [
        ("IdLocation", "@x"),
        ("DataLocation", "@(\"^\"_g)"),
        ("IndexLocation", "$name(^X(1))"),
        ("StreamLocation", "$NA(^X)"),
        ("CounterLocation", "$NA(^IadX)"),
        ("SqlIdExpression", "$i(@g)"),
    ] {
        let r = gate(&map_with(vec![leaf(tag, value)]), edit(), &p);
        assert_eq!(
            code_of(&r),
            "STORAGE_MAP_DYNAMIC_GLOBAL_BLOCKED",
            "{tag} {value}: {r:?}"
        );
    }
}

#[test]
fn a_global_inside_a_location_subscript_goes_through_the_gates() {
    let mut p = EffectivePolicy::default();
    p.global_blocklist.value = vec!["^IadSecret*".to_string()];
    let r = gate(
        &map_with(vec![leaf("DataLocation", "^IadX(^IadSecretY)")]),
        edit(),
        &p,
    );
    assert_eq!(code_of(&r), "SYSTEM_BLOCKLIST", "{r:?}");
    let r = gate(
        &map_with(vec![leaf("IdLocation", "IadX($G(^PAPMI(1)))")]),
        edit(),
        &p,
    );
    assert_eq!(code_of(&r), "PHI_GATE_BLOCKED", "{r:?}");
}

#[test]
fn a_phi_global_inside_a_dynamic_location_is_refused_by_the_phi_gate() {
    let p = EffectivePolicy::default();
    let r = gate(
        &map_with(vec![leaf("CounterLocation", "$NA(^PAPMI)")]),
        edit(),
        &p,
    );
    assert_eq!(code_of(&r), "PHI_GATE_BLOCKED", "{r:?}");
}
