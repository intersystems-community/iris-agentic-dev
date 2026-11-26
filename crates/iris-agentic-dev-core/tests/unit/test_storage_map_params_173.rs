// Spec 173, T012 and T014: the new `propose` params parse from JSON text, a misspelt key is
// refused, and each param is allowed only where it means something.

use iris_agentic_dev_core::tools::params::batch8::IrisStorageMapParams;
use iris_agentic_dev_core::tools::storage_map::param_use_error;
use serde_json::json;

fn parse(text: &str) -> Result<IrisStorageMapParams, serde_json::Error> {
    serde_json::from_str(text)
}

#[test]
fn index_globals_and_field_names_parse_from_json_text() {
    let p = parse(
        r#"{"action":"propose","global":"^bdb","index_globals":["^bdb.x","bdb.y"],
            "field_names":["Name","City"]}"#,
    )
    .expect("parses");
    assert_eq!(
        p.index_globals.as_deref(),
        Some(&["^bdb.x".to_string(), "bdb.y".to_string()][..])
    );
    assert_eq!(
        p.field_names.as_deref(),
        Some(&["Name".to_string(), "City".to_string()][..])
    );
}

#[test]
fn class_on_propose_parses() {
    let p = parse(r#"{"action":"propose","class":"Pkg.Member"}"#).expect("parses");
    assert_eq!(p.class.as_deref(), Some("Pkg.Member"));
    assert!(p.index_globals.is_none() && p.field_names.is_none());
}

#[test]
fn a_misspelt_key_is_refused() {
    for key in ["index_global", "indexGlobals", "field_name", "fieldNames"] {
        let text = format!(r#"{{"action":"propose","global":"^G","{key}":["x"]}}"#);
        let e = parse(&text).expect_err(key).to_string();
        assert!(e.contains("unknown field"), "{key}: {e}");
    }
}

#[test]
fn index_globals_must_be_an_array_of_strings() {
    assert!(parse(r#"{"action":"propose","global":"^G","index_globals":"^G.x"}"#).is_err());
    assert!(parse(r#"{"action":"propose","global":"^G","field_names":[1,2]}"#).is_err());
}

// T014: where each param may appear.

#[test]
fn index_globals_and_field_names_are_for_propose_only() {
    for action in [
        "read", "diagram", "uses", "compare", "preview", "check", "edit", "replace",
    ] {
        for (key, value) in [
            ("index_globals", json!(["^G.x"])),
            ("field_names", json!(["Name"])),
        ] {
            let p = json!({"action": action, key: value});
            let e = param_use_error(&p).unwrap_or_else(|| panic!("{action} {key}"));
            assert!(e.contains(key) && e.contains(action), "{e}");
        }
    }
    // A null or absent value is not a use (the handler builds its JSON from every declared field).
    let p = json!({"action": "read", "index_globals": null, "field_names": null});
    assert_eq!(param_use_error(&p), None);
}

#[test]
fn propose_takes_global_or_class_never_both_never_neither() {
    let both = json!({"action": "propose", "global": "^G", "class": "A.B"});
    let e = param_use_error(&both).expect("both");
    assert!(e.contains("global") && e.contains("class"), "{e}");

    let neither = json!({"action": "propose", "class": null, "global": null});
    let e = param_use_error(&neither).expect("neither");
    assert!(e.contains("global") && e.contains("class"), "{e}");

    assert_eq!(
        param_use_error(&json!({"action": "propose", "global": "^G"})),
        None
    );
    assert_eq!(
        param_use_error(&json!({"action": "propose", "class": "A.B"})),
        None
    );
}

#[test]
fn field_names_do_not_go_with_class() {
    // A class names its own properties.
    let p = json!({"action": "propose", "class": "A.B", "field_names": ["x"]});
    let e = param_use_error(&p).expect("class with field_names");
    assert!(e.contains("field_names") && e.contains("class"), "{e}");
}

#[test]
fn other_actions_keep_their_class_and_global() {
    // `class` with `global` is fine on `uses`/`compare`: only `propose` reads them as alternatives.
    assert_eq!(
        param_use_error(&json!({"action": "compare", "class": "A.B", "global": "^G"})),
        None
    );
}
