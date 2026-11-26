//! Spec 173 T025: the generated linker is built from validated, quoted names, calls the server's
//! own collation functions, and writes only JSON lines of counts, positions and names.

use iris_agentic_dev_core::tools::storage_map::link_code::{
    build_code, Caps, FieldAccess, LinkRequest, FUNCTION_METHODS,
};

fn fields() -> Vec<FieldAccess> {
    (1..=5)
        .map(|piece| FieldAccess {
            node: None,
            delimiter: Some(94),
            piece,
        })
        .collect()
}

fn request<'a>(
    candidate: &'a str,
    prefix: &'a [String],
    fields: &'a [FieldAccess],
) -> LinkRequest<'a> {
    LinkRequest {
        master: "^IadSm173Person",
        candidate,
        sub_prefix: prefix,
        key_parts: 1,
        fields,
        caps: Caps::for_node_cap(200),
        with_values: false,
    }
}

#[test]
fn a_name_that_is_not_a_global_builds_nothing() {
    let f = fields();
    for bad in [
        "^X) Kill ^Y",
        "^A(\"x\")",
        "^A B",
        "^|\"NS\"|G",
        "",
        "^1abc",
    ] {
        assert!(build_code(&request(bad, &[], &f)).is_err(), "{bad:?}");
    }
    let mut r = request("^IadSm173PersonX", &[], &f);
    r.master = "^Per son";
    assert!(build_code(&r).is_err());
}

#[test]
fn a_good_request_quotes_both_globals() {
    let f = fields();
    let code = build_code(&request("^IadSm173PersonX", &[], &f)).unwrap();
    assert!(code.contains("Set mst=\"^IadSm173Person\""), "{code}");
    assert!(code.contains("cand=\"^IadSm173PersonX\""), "{code}");
}

#[test]
fn a_sub_prefix_is_quoted_not_spliced() {
    let f = fields();
    let prefix = vec!["idx\"); Kill ^X ;(\"".to_string()];
    let code = build_code(&request("^IadSm173Person", &prefix, &f)).unwrap();
    assert!(
        code.contains("$Name(@cand@(\"idx\"\"); Kill ^X ;(\"\"\"))"),
        "{code}"
    );
    assert!(
        !code.contains("idx\"); Kill"),
        "the hostile text escaped its quotes: {code}"
    );
}

#[test]
fn every_transform_goes_through_the_servers_function_class() {
    let f = fields();
    let code = build_code(&request("^IadSm173PersonX", &[], &f)).unwrap();
    assert!(code.contains("%SYSTEM.SQL.Functions"), "{code}");
    for m in FUNCTION_METHODS {
        assert!(code.contains(&format!("\"{m}\"")), "{m} missing: {code}");
    }
    assert_eq!(FUNCTION_METHODS.len(), 7);
    // iad implements no collation of its own: no internal function number, no case conversion.
    for banned in ["$ZU", "$ZCVT", "$ZCONVERT", "$ZUTIL", "$$$UPPER"] {
        assert!(
            !code.to_uppercase().contains(banned),
            "{banned} appears: {code}"
        );
    }
}

#[test]
fn the_only_writes_are_json_lines_of_counts_and_names() {
    let f = fields();
    let code = build_code(&request("^IadSm173PersonX", &[], &f)).unwrap();
    let writes: Vec<&str> = code
        .match_indices("Write ")
        .map(|(i, _)| &code[i..])
        .collect();
    assert!(writes.len() >= 2, "a sample line and a done line: {code}");
    for w in writes {
        let stmt = w.lines().next().unwrap();
        assert!(
            stmt.starts_with("Write o.%ToJSON(),!"),
            "a write that is not a JSON line: {stmt}"
        );
    }
    // The lines carry a type tag.
    assert!(code.contains("o.t=\"sample\""));
    assert!(code.contains("o.t=\"done\""));
}

#[test]
fn the_spread_walk_and_the_caps_are_there() {
    let f = fields();
    let mut r = request("^IadSm173PersonX", &[], &f);
    r.caps = Caps::for_node_cap(300);
    let code = build_code(&r).unwrap();
    assert!(code.contains("$Order(@cand@(k))"), "{code}");
    assert!(code.contains("nodecap=300"), "{code}");
    assert!(
        code.contains("ccap=3000"),
        "the counting pass is 10x: {code}"
    );
    assert!(code.contains("secs=5"), "{code}");
    assert!(code.contains("$ZH-t0"), "the time cap: {code}");
    assert!(code.contains("count_cap"), "{code}");
}

#[test]
fn caps_for_a_node_cap_give_the_counting_pass_ten_times() {
    let c = Caps::for_node_cap(1000);
    assert_eq!((c.node_cap, c.count_cap, c.time_cap_secs), (1000, 10000, 5));
}

#[test]
fn a_field_table_entry_per_master_field() {
    let mut f = fields();
    f[2].delimiter = None; // a $LIST element
    f[3].node = Some("EV".into());
    let code = build_code(&request("^IadSm173PersonX", &[], &f)).unwrap();
    assert!(code.contains("Set nf=5"), "{code}");
    assert!(code.contains("Set fa(3)=$ListBuild(\"\",0,3)"), "{code}");
    assert!(code.contains("Set fa(4)=$ListBuild(\"EV\",94,4)"), "{code}");
    assert!(build_code(&request("^X", &[], &[])).is_err());
}
