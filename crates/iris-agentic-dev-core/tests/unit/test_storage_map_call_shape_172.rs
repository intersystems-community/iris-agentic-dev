//! Storage map call_shape tests (T034, T035, T036)

use iris_agentic_dev_core::tools::exec::needs_service_account;
use iris_agentic_dev_core::tools::storage_map::{call_shape, Action};
use iris_agentic_dev_core::tools::write_gate::{classify, is_scratch_call, WriteClass};
use serde_json::json;

#[test]
fn test_call_shape_all_actions() {
    // Test each action
    let params = json!({"action": "read"});
    let shape = call_shape(&params);
    assert_eq!(shape.action, Action::Read);
    assert!(!shape.seed_counter);
    assert!(!shape.build_indexes);

    let params = json!({"action": "diagram"});
    assert_eq!(call_shape(&params).action, Action::Diagram);

    let params = json!({"action": "uses"});
    assert_eq!(call_shape(&params).action, Action::Uses);

    let params = json!({"action": "compare"});
    assert_eq!(call_shape(&params).action, Action::Compare);

    let params = json!({"action": "preview"});
    assert_eq!(call_shape(&params).action, Action::Preview);

    let params = json!({"action": "propose"});
    assert_eq!(call_shape(&params).action, Action::Propose);

    let params = json!({"action": "check"});
    assert_eq!(call_shape(&params).action, Action::Check);

    let params = json!({"action": "edit"});
    assert_eq!(call_shape(&params).action, Action::Edit);

    let params = json!({"action": "replace"});
    assert_eq!(call_shape(&params).action, Action::Replace);
}

/// T034: a case or whitespace variant is not the action it resembles. `"Read"` must not be
/// judged as a read, because the handler would not run it as one.
#[test]
fn test_call_shape_case_and_whitespace_variants_are_unreadable() {
    for a in [
        "Read", "READ", " read", "read ", " EDIT ", "Edit", "dIagram", "uses\n", "edit\t",
    ] {
        let shape = call_shape(&json!({"action": a, "acknowledgePhi": true}));
        assert!(shape.unreadable, "{a:?}");
        assert_eq!(shape.action, Action::Edit, "{a:?}");
        assert!(shape.seed_counter && shape.build_indexes, "{a:?}");
        assert!(
            !shape.acknowledge_phi,
            "an unreadable call acknowledges nothing: {a:?}"
        );
    }
    let shape = call_shape(&json!({"action": "read", "acknowledgePhi": true}));
    assert!(!shape.unreadable);
    assert_eq!(shape.action, Action::Read);
    assert!(shape.acknowledge_phi);
    assert!(!call_shape(&json!({"action": "read", "acknowledgePhi": "true"})).acknowledge_phi);
}

#[test]
fn test_call_shape_unreadable() {
    // Missing action becomes Edit with opt-ins true
    let params = json!({});
    let shape = call_shape(&params);
    assert_eq!(shape.action, Action::Edit);
    assert!(shape.seed_counter);
    assert!(shape.build_indexes);

    // Number instead of string becomes Edit with opt-ins
    let params = json!({"action": 123});
    let shape = call_shape(&params);
    assert_eq!(shape.action, Action::Edit);
    assert!(shape.seed_counter);
    assert!(shape.build_indexes);

    // Unknown string becomes Edit with opt-ins
    let params = json!({"action": "unknown"});
    let shape = call_shape(&params);
    assert_eq!(shape.action, Action::Edit);
    assert!(shape.seed_counter);
    assert!(shape.build_indexes);
}

#[test]
fn test_call_shape_opt_ins() {
    // Opt-ins as true
    let params = json!({
        "action": "edit",
        "seed_counter": true,
        "build_indexes": true,
        "include_values": true
    });
    let shape = call_shape(&params);
    assert_eq!(shape.action, Action::Edit);
    assert!(shape.seed_counter);
    assert!(shape.build_indexes);
    assert!(shape.include_values);

    // Opt-ins as false
    let params = json!({
        "action": "edit",
        "seed_counter": false,
        "build_indexes": false,
        "include_values": false
    });
    let shape = call_shape(&params);
    assert!(!shape.seed_counter);
    assert!(!shape.build_indexes);
    assert!(!shape.include_values);

    // Opt-ins absent default to false
    let params = json!({"action": "edit"});
    let shape = call_shape(&params);
    assert!(!shape.seed_counter);
    assert!(!shape.build_indexes);
    assert!(!shape.include_values);

    // String "true" is NOT true (only boolean true counts)
    let params = json!({
        "action": "edit",
        "seed_counter": "true",
        "build_indexes": "true"
    });
    let shape = call_shape(&params);
    assert!(!shape.seed_counter);
    assert!(!shape.build_indexes);
}

// ── T035: classify integration ─────────────────────────────────────────────────

#[test]
fn test_classify_iris_storage_map_read() {
    let args = json!({"action": "read"}).as_object().cloned();
    let class = classify("iris_storage_map", args.as_ref());
    assert_eq!(class, Some(WriteClass::ReadOnly));
}

#[test]
fn test_classify_iris_storage_map_diagram() {
    let args = json!({"action": "diagram"}).as_object().cloned();
    let class = classify("iris_storage_map", args.as_ref());
    assert_eq!(class, Some(WriteClass::ReadOnly));
}

#[test]
fn test_classify_iris_storage_map_uses() {
    let args = json!({"action": "uses"}).as_object().cloned();
    let class = classify("iris_storage_map", args.as_ref());
    assert_eq!(class, Some(WriteClass::ReadOnly));
}

#[test]
fn test_classify_iris_storage_map_check() {
    let args = json!({"action": "check"}).as_object().cloned();
    let class = classify("iris_storage_map", args.as_ref());
    assert_eq!(class, Some(WriteClass::ReadOnly));
}

#[test]
fn test_classify_iris_storage_map_edit() {
    let args = json!({"action": "edit"}).as_object().cloned();
    let class = classify("iris_storage_map", args.as_ref());
    assert_eq!(class, Some(WriteClass::Write));
}

#[test]
fn test_classify_iris_storage_map_replace() {
    let args = json!({"action": "replace"}).as_object().cloned();
    let class = classify("iris_storage_map", args.as_ref());
    assert_eq!(class, Some(WriteClass::Write));
}

#[test]
fn test_classify_iris_storage_map_compare() {
    let args = json!({"action": "compare"}).as_object().cloned();
    let class = classify("iris_storage_map", args.as_ref());
    assert_eq!(class, Some(WriteClass::Write));
}

#[test]
fn test_classify_iris_storage_map_preview() {
    let args = json!({"action": "preview"}).as_object().cloned();
    let class = classify("iris_storage_map", args.as_ref());
    assert_eq!(class, Some(WriteClass::Write));
}

#[test]
fn test_classify_iris_storage_map_propose() {
    let args = json!({"action": "propose"}).as_object().cloned();
    let class = classify("iris_storage_map", args.as_ref());
    assert_eq!(class, Some(WriteClass::Write));
}

#[test]
fn test_classify_iris_storage_map_unreadable() {
    // Unreadable action becomes Edit with opt-ins (seed_counter + build_indexes = true)
    // Edit with build_indexes is Destructive
    let args = json!({"action": "unknown"}).as_object().cloned();
    let class = classify("iris_storage_map", args.as_ref());
    assert_eq!(class, Some(WriteClass::Destructive));
}

// ── T035: is_scratch_call integration ──────────────────────────────────────────

#[test]
fn test_is_scratch_call_read() {
    let args = json!({"action": "read"}).as_object().cloned();
    assert!(!is_scratch_call("iris_storage_map", args.as_ref()));
}

#[test]
fn test_is_scratch_call_diagram() {
    let args = json!({"action": "diagram"}).as_object().cloned();
    assert!(!is_scratch_call("iris_storage_map", args.as_ref()));
}

#[test]
fn test_is_scratch_call_uses() {
    let args = json!({"action": "uses"}).as_object().cloned();
    assert!(!is_scratch_call("iris_storage_map", args.as_ref()));
}

#[test]
fn test_is_scratch_call_check() {
    let args = json!({"action": "check"}).as_object().cloned();
    assert!(is_scratch_call("iris_storage_map", args.as_ref()));
}

#[test]
fn test_is_scratch_call_edit() {
    let args = json!({"action": "edit"}).as_object().cloned();
    assert!(is_scratch_call("iris_storage_map", args.as_ref()));
}

#[test]
fn test_is_scratch_call_replace() {
    let args = json!({"action": "replace"}).as_object().cloned();
    assert!(is_scratch_call("iris_storage_map", args.as_ref()));
}

#[test]
fn test_is_scratch_call_compare() {
    let args = json!({"action": "compare"}).as_object().cloned();
    assert!(is_scratch_call("iris_storage_map", args.as_ref()));
}

#[test]
fn test_is_scratch_call_preview() {
    let args = json!({"action": "preview"}).as_object().cloned();
    assert!(is_scratch_call("iris_storage_map", args.as_ref()));
}

#[test]
fn test_is_scratch_call_propose() {
    let args = json!({"action": "propose"}).as_object().cloned();
    assert!(is_scratch_call("iris_storage_map", args.as_ref()));
}

// ── T036: needs_service_account integration ────────────────────────────────────

#[test]
fn test_needs_service_account_read() {
    let args = json!({"action": "read"});
    assert!(!needs_service_account("iris_storage_map", &args));
}

#[test]
fn test_needs_service_account_diagram() {
    let args = json!({"action": "diagram"});
    assert!(!needs_service_account("iris_storage_map", &args));
}

#[test]
fn test_needs_service_account_uses() {
    let args = json!({"action": "uses"});
    assert!(!needs_service_account("iris_storage_map", &args));
}

#[test]
fn test_needs_service_account_check() {
    let args = json!({"action": "check"});
    assert!(!needs_service_account("iris_storage_map", &args));
}

#[test]
fn test_needs_service_account_edit() {
    // FR-029 (amended 2026-11-14): its data checks read the map's globals through iad's code.
    let args = json!({"action": "edit"});
    assert!(needs_service_account("iris_storage_map", &args));
}

#[test]
fn test_needs_service_account_replace() {
    // FR-029 (amended 2026-11-14): its data checks read the map's globals through iad's code.
    let args = json!({"action": "replace"});
    assert!(needs_service_account("iris_storage_map", &args));
}

#[test]
fn test_needs_service_account_compare() {
    let args = json!({"action": "compare"});
    assert!(needs_service_account("iris_storage_map", &args));
}

#[test]
fn test_needs_service_account_preview() {
    let args = json!({"action": "preview"});
    assert!(needs_service_account("iris_storage_map", &args));
}

#[test]
fn test_needs_service_account_propose() {
    let args = json!({"action": "propose"});
    assert!(needs_service_account("iris_storage_map", &args));
}

#[test]
fn test_needs_service_account_with_seed_counter() {
    let args = json!({"action": "edit", "seed_counter": true});
    assert!(needs_service_account("iris_storage_map", &args));
}

#[test]
fn test_needs_service_account_with_build_indexes() {
    let args = json!({"action": "replace", "build_indexes": true});
    assert!(needs_service_account("iris_storage_map", &args));
}

#[test]
fn classify_with_no_arguments_is_destructive_and_scratch() {
    use iris_agentic_dev_core::tools::write_gate::{classify, is_scratch_call, WriteClass};
    assert_eq!(
        classify("iris_storage_map", None),
        Some(WriteClass::Destructive)
    );
    assert!(is_scratch_call("iris_storage_map", None));
}

// ── T064: opt-ins and map code raise the call (plan "Classification") ──────────

mod t064 {
    use iris_agentic_dev_core::iris::workspace_config::ToolCategory::{
        self, Compile, Docs, Execute,
    };
    use iris_agentic_dev_core::tools::storage_map::call_shape;
    use iris_agentic_dev_core::tools::write_gate::{classify, effective_category, WriteClass};
    use serde_json::{json, Value};

    fn judged(args: Value) -> (WriteClass, Vec<ToolCategory>) {
        let a = args.as_object().cloned();
        let class = classify("iris_storage_map", a.as_ref()).expect("classified");
        let mut cats = effective_category("iris_storage_map", a.as_ref(), class);
        cats.sort_by_key(|c| format!("{c:?}"));
        (class, cats)
    }

    /// A literal-only edit op (a piece number).
    fn literal_op() -> Value {
        json!([{"op": "set", "path": "/maps/Master/data/Age/piece", "value": "4"}])
    }

    #[test]
    fn t064_read_diagram_uses_are_read_only_docs() {
        for a in ["read", "diagram", "uses"] {
            assert_eq!(
                judged(json!({"action": a})),
                (WriteClass::ReadOnly, vec![Docs]),
                "{a}"
            );
        }
    }

    #[test]
    fn t064_check_is_read_only_compile() {
        assert_eq!(
            judged(json!({"action": "check", "model": {"global": "^G"}})),
            (WriteClass::ReadOnly, vec![Compile])
        );
    }

    #[test]
    fn t064_compare_propose_preview_are_write_execute() {
        for a in ["compare", "propose", "preview"] {
            assert_eq!(
                judged(json!({"action": a})),
                (WriteClass::Write, vec![Execute]),
                "{a}"
            );
        }
    }

    #[test]
    fn t064_edit_replace_without_code_are_write_compile() {
        assert_eq!(
            judged(json!({"action": "edit", "ops": literal_op()})),
            (WriteClass::Write, vec![Compile])
        );
        let model = json!({"name": "Master", "type": "data", "data": {"Age": {"name": "Age",
                    "delimiter": "\"^\"", "piece": "3"}}});
        assert_eq!(
            judged(json!({"action": "replace", "model": model})),
            (WriteClass::Write, vec![Compile])
        );
    }

    #[test]
    fn t064_edit_replace_with_map_code_add_execute() {
        let ops = json!([{"op": "set", "path": "/maps/Master/data/Age/retrieval_code",
                          "value": "Set {*}=$i(^Side)"}]);
        assert_eq!(
            judged(json!({"action": "edit", "ops": ops})),
            (WriteClass::Write, vec![Compile, Execute])
        );
        let model = json!({"name": "Master", "condition": "$i(^Side)"});
        assert_eq!(
            judged(json!({"action": "replace", "model": model})),
            (WriteClass::Write, vec![Compile, Execute])
        );
        // A subtree set whose contents carry code is code too.
        let ops = json!([{"op": "set", "path": "/maps/Master/subscripts/1",
                          "value": {"level": 1, "expression": "{ID}"}}]);
        assert_eq!(
            judged(json!({"action": "edit", "ops": ops})).1,
            vec![Compile, Execute]
        );
        // Removing a code element changes what runs.
        let ops = json!([{"op": "remove", "path": "/maps/Master/condition"}]);
        assert_eq!(
            judged(json!({"action": "edit", "ops": ops})).1,
            vec![Compile, Execute]
        );
        // An unknown key is judged as code (fail closed, like `code::code_elements`).
        let ops = json!([{"op": "set", "path": "/maps/Master/wibble", "value": "x"}]);
        assert_eq!(
            judged(json!({"action": "edit", "ops": ops})).1,
            vec![Compile, Execute]
        );
    }

    #[test]
    fn t064_extra_elements_are_judged_by_their_tag() {
        let model = |tag: &str| json!({"name": "Master", "extra": [{"tag": tag, "value": "x"}]});
        assert_eq!(
            judged(json!({"action": "replace", "model": model("Structure")})).1,
            vec![Compile],
            "a literal tag carries no code"
        );
        assert_eq!(
            judged(json!({"action": "replace", "model": model("Wibble")})).1,
            vec![Compile, Execute],
            "an unknown tag is code"
        );
    }

    #[test]
    fn t064_seed_counter_adds_execute() {
        assert_eq!(
            judged(json!({"action": "edit", "ops": literal_op(), "seed_counter": true})),
            (WriteClass::Write, vec![Compile, Execute])
        );
    }

    #[test]
    fn t064_build_indexes_is_destructive_and_adds_execute() {
        assert_eq!(
            judged(json!({"action": "replace", "model": {"name": "M"}, "build_indexes": true})),
            (WriteClass::Destructive, vec![Compile, Execute])
        );
    }

    #[test]
    fn t064_unreadable_is_the_strictest_row() {
        assert_eq!(
            judged(json!({"action": "Edit"})),
            (WriteClass::Destructive, vec![Compile, Execute])
        );
    }

    #[test]
    fn t064_call_shape_sets_code_changed_for_writes_only() {
        assert_eq!(call_shape(&json!({"action": "read"})).code_changed, None);
        assert_eq!(
            call_shape(&json!({"action": "check", "model": {"condition": "1"}})).code_changed,
            None
        );
        assert_eq!(
            call_shape(&json!({"action": "edit", "ops": literal_op()})).code_changed,
            Some(false)
        );
        assert_eq!(
            call_shape(&json!({"action": "edit", "ops": "not a list"})).code_changed,
            Some(true),
            "ops iad cannot read are judged as code"
        );
        assert_eq!(call_shape(&json!({})).code_changed, Some(true));
    }
}
