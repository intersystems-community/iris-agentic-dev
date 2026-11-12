//! T090 (FR-024): the ID counter a map's `SqlIdExpression` increments, when iad can tell it for
//! sure; anything else is reported as not verifiable, with why.

use iris_agentic_dev_core::tools::storage_map::write::id_counter_ref;
use std::collections::BTreeMap;

fn kw(expr: Option<&str>) -> BTreeMap<String, String> {
    let mut m = BTreeMap::new();
    if let Some(e) = expr {
        m.insert("SqlIdExpression".to_string(), e.to_string());
    }
    m.insert("StreamLocation".to_string(), "^XS".to_string());
    m
}

#[test]
fn test_storage_map_counter_172_plain_forms() {
    let r = id_counter_ref(&kw(Some("$i(^IadSm172Member)"))).expect("plain $i");
    assert_eq!(r.name, "^IadSm172Member");
    assert!(r.subscripts_prefix.is_none());
    let r = id_counter_ref(&kw(Some("$INCREMENT(^App.Data(\"ids\"))")))
        .expect("$INCREMENT with a string");
    assert_eq!(r.name, "^App.Data");
    assert_eq!(r.subscripts_prefix.as_deref(), Some("(\"ids\")"));
    let r = id_counter_ref(&kw(Some("$i(^G(\"my ids\", 2))"))).expect("a string with a space");
    assert_eq!(r.subscripts_prefix.as_deref(), Some("(\"my ids\",2)"));
    let r = id_counter_ref(&kw(Some(" $I( ^G(1,2) ) "))).expect("spaces and two numbers");
    assert_eq!(
        (r.name.as_str(), r.subscripts_prefix.as_deref()),
        ("^G", Some("(1,2)"))
    );
}

#[test]
fn test_storage_map_counter_172_not_verifiable() {
    for (expr, why) in [
        (Some("$$NextId^MyRtn()"), "a function call"),
        (Some("$i(^G(%d(1)))"), "a subscript that is not a literal"),
        (Some("$i(@g)"), "indirection"),
        (Some("$i(^|\"NS\"|G)"), "an extended reference"),
        (Some("$i(^G)+1"), "more than the increment"),
        (None, "no SqlIdExpression"),
    ] {
        let err = id_counter_ref(&kw(expr)).expect_err(why);
        assert!(!err.is_empty(), "{why}");
    }
}
