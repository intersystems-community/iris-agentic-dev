//! Spec 173, T022: the datatype guess for each property of the class draft, the `COLLATION`
//! parameter the linking evidence gives, and the class source built from them. The evidence is
//! counts only, so a sampled value cannot reach the source: the type has no field to hold one.

use iris_agentic_dev_core::tools::storage_map::typing::{
    class_source, collation_parameter, property_type, Evidence, PropertySpec,
};

fn evidence(of: u32) -> Evidence {
    Evidence {
        of,
        ..Evidence::default()
    }
}

fn spec(name: &str, e: Evidence, collation: Option<&str>) -> PropertySpec {
    PropertySpec {
        name: name.to_string(),
        ty: property_type(&e),
        collation: collation.map(String::from),
    }
}

// ---------------------------------------------------------------------------
// The evidence holds counts and no value
// ---------------------------------------------------------------------------

#[test]
fn test_typing_173_evidence_is_counts_only() {
    // An exhaustive pattern: adding a field (a value, a sample) breaks this test's compile, so
    // the change is a decision, not an accident.
    let Evidence {
        of,
        integer,
        numeric,
        max_scale,
        date_fit,
        max_length,
    } = Evidence::default();
    let counts: [u32; 6] = [of, integer, numeric, max_scale, date_fit, max_length];
    assert_eq!(counts, [0; 6]);
}

// ---------------------------------------------------------------------------
// property_type
// ---------------------------------------------------------------------------

#[test]
fn test_typing_173_two_hundred_of_two_hundred_integers_is_percent_integer() {
    let t = property_type(&Evidence {
        of: 200,
        integer: 200,
        numeric: 200,
        ..Evidence::default()
    });
    assert_eq!(t.name, "%Integer");
    assert!(t.parameters.is_empty());
    assert!(t.date_guess.is_none());
    assert_eq!(t.evidence, serde_json::json!({"integer": 200, "of": 200}));
}

#[test]
fn test_typing_173_one_non_integer_among_integers_is_not_percent_integer() {
    let t = property_type(&Evidence {
        of: 200,
        integer: 199,
        numeric: 199,
        max_length: 5,
        ..Evidence::default()
    });
    assert_eq!(t.name, "%String", "a non-number in the column is text");
}

#[test]
fn test_typing_173_decimals_are_numeric_with_the_scale_of_the_longest_fraction() {
    let t = property_type(&Evidence {
        of: 200,
        integer: 40,
        numeric: 200,
        max_scale: 2,
        max_length: 7,
        ..Evidence::default()
    });
    assert_eq!(t.name, "%Numeric");
    assert_eq!(t.parameters, ["SCALE = 2"]);
    assert_eq!(
        t.evidence,
        serde_json::json!({"numeric": 200, "max_scale": 2, "of": 200})
    );
    let t = property_type(&Evidence {
        of: 10,
        integer: 0,
        numeric: 10,
        max_scale: 1,
        ..Evidence::default()
    });
    assert_eq!(t.parameters, ["SCALE = 1"]);
}

#[test]
fn test_typing_173_a_date_guess_needs_95_percent_and_reports_the_fit_count() {
    // 190 of 200 is exactly 95%.
    let t = property_type(&Evidence {
        of: 200,
        integer: 200,
        numeric: 200,
        date_fit: 190,
        ..Evidence::default()
    });
    assert_eq!(t.name, "%Date");
    let g = t.date_guess.as_ref().expect("a date guess");
    assert_eq!((g.fit, g.of), (190, 200));
    assert_eq!(t.evidence, serde_json::json!({"date_fit": 190, "of": 200}));
    // 189 of 200 is below it: whole numbers, not dates.
    let t = property_type(&Evidence {
        of: 200,
        integer: 200,
        numeric: 200,
        date_fit: 189,
        ..Evidence::default()
    });
    assert_eq!(t.name, "%Integer");
    assert!(t.date_guess.is_none());
}

#[test]
fn test_typing_173_maxlen_rounds_up_to_the_next_size_from_the_longest_seen() {
    for (longest, maxlen) in [
        (0, 32),
        (1, 32),
        (31, 32),
        (32, 32),
        (33, 64),
        (64, 64),
        (65, 128),
        (128, 128),
        (129, 255),
        (255, 255),
        (256, 1024),
        (1024, 1024),
    ] {
        let t = property_type(&Evidence {
            max_length: longest,
            ..evidence(50)
        });
        assert_eq!(t.name, "%String", "longest {longest}");
        assert_eq!(
            t.parameters,
            [format!("MAXLEN = {maxlen}")],
            "longest {longest}"
        );
        assert_eq!(t.evidence, serde_json::json!({"max_length": longest}));
    }
}

#[test]
fn test_typing_173_text_longer_than_the_largest_size_rounds_to_a_multiple_of_it() {
    let t = property_type(&Evidence {
        max_length: 1025,
        ..evidence(5)
    });
    assert_eq!(t.parameters, ["MAXLEN = 2048"]);
}

#[test]
fn test_typing_173_no_values_is_a_short_string() {
    let t = property_type(&Evidence::default());
    assert_eq!(t.name, "%String");
    assert_eq!(t.parameters, ["MAXLEN = 32"]);
}

// ---------------------------------------------------------------------------
// collation_parameter
// ---------------------------------------------------------------------------

#[test]
fn test_typing_173_sqlupper_is_the_default_so_it_gives_no_parameter() {
    assert_eq!(collation_parameter("sqlupper", None).unwrap(), None);
}

#[test]
fn test_typing_173_the_named_collations() {
    for (transform, want) in [
        ("exact", "COLLATION = \"EXACT\""),
        ("sqlstring", "COLLATION = \"SQLSTRING\""),
        ("string", "COLLATION = \"STRING\""),
    ] {
        assert_eq!(
            collation_parameter(transform, None).unwrap().as_deref(),
            Some(want),
            "{transform}"
        );
    }
}

#[test]
fn test_typing_173_a_length_variant_carries_its_length() {
    assert_eq!(
        collation_parameter("sqlupper", Some(4)).unwrap().as_deref(),
        Some("COLLATION = \"SQLUPPER(4)\"")
    );
    assert_eq!(
        collation_parameter("sqlstring", Some(10))
            .unwrap()
            .as_deref(),
        Some("COLLATION = \"SQLSTRING(10)\"")
    );
    assert_eq!(
        collation_parameter("string", Some(3)).unwrap().as_deref(),
        Some("COLLATION = \"STRING(3)\"")
    );
    assert_eq!(
        collation_parameter("truncate", Some(3)).unwrap().as_deref(),
        Some("COLLATION = \"TRUNCATE(3)\"")
    );
}

#[test]
fn test_typing_173_the_other_closed_set_transforms() {
    for (transform, want) in [
        ("alphaup", "COLLATION = \"ALPHAUP\""),
        ("upper", "COLLATION = \"UPPER\""),
        ("mvr", "COLLATION = \"MVR\""),
    ] {
        assert_eq!(
            collation_parameter(transform, None).unwrap().as_deref(),
            Some(want),
            "{transform}"
        );
    }
}

#[test]
fn test_typing_173_collation_refuses_what_it_cannot_spell() {
    for (transform, length) in [
        ("collate", None),
        ("SQLUPPER", None),
        ("", None),
        ("truncate", None),
        ("truncate", Some(0)),
        ("sqlupper", Some(0)),
        ("exact", Some(3)),
        ("mvr", Some(3)),
    ] {
        assert!(
            collation_parameter(transform, length).is_err(),
            "{transform:?} {length:?} must be refused"
        );
    }
}

// ---------------------------------------------------------------------------
// class_source
// ---------------------------------------------------------------------------

#[test]
fn test_typing_173_class_source_lists_every_property_with_its_type_and_collation() {
    let props = vec![
        spec(
            "Age",
            Evidence {
                of: 200,
                integer: 200,
                numeric: 200,
                ..Evidence::default()
            },
            None,
        ),
        spec(
            "Name",
            Evidence {
                max_length: 31,
                ..evidence(200)
            },
            Some("COLLATION = \"EXACT\""),
        ),
        spec(
            "Balance",
            Evidence {
                of: 200,
                numeric: 200,
                max_scale: 2,
                ..Evidence::default()
            },
            None,
        ),
    ];
    let draft = class_source("IadSm173.Person", &props).unwrap();
    let s = &draft.source;
    assert!(
        s.starts_with("Class IadSm173.Person Extends %Persistent"),
        "{s}"
    );
    assert!(s.contains("Property Age As %Integer;"), "{s}");
    assert!(
        s.contains("Property Name As %String(MAXLEN = 32, COLLATION = \"EXACT\");"),
        "{s}"
    );
    assert!(
        s.contains("Property Balance As %Numeric(SCALE = 2);"),
        "{s}"
    );
    assert!(s.trim_end().ends_with('}'), "{s}");
    assert_eq!(
        draft.guesses,
        [
            "/class_draft/properties/Age",
            "/class_draft/properties/Name",
            "/class_draft/properties/Balance"
        ]
    );
}

#[test]
fn test_typing_173_class_source_holds_counts_and_names_and_no_sample() {
    // Evidence carries only counts, so the source is built from counts, names and fixed words.
    // The one numbers in it are the type parameters and the evidence comments.
    let props = vec![spec(
        "When",
        Evidence {
            of: 200,
            integer: 200,
            numeric: 200,
            date_fit: 198,
            ..Evidence::default()
        },
        None,
    )];
    let draft = class_source("X.Y", &props).unwrap();
    assert!(draft.source.contains("Property When As %Date;"));
    assert!(
        draft.source.contains("198 of 200"),
        "the fit count is reported: {}",
        draft.source
    );
    let json = draft.to_json(true);
    assert_eq!(json["compiled"], true);
    assert_eq!(json["source"], draft.source);
    assert_eq!(json["properties"][0]["name"], "When");
    assert_eq!(json["properties"][0]["type"], "%Date");
    assert_eq!(
        json["properties"][0]["evidence"],
        serde_json::json!({"date_fit": 198, "of": 200})
    );
    assert_eq!(json["guesses"][0], "/class_draft/properties/When");
    assert_eq!(draft.to_json(false)["compiled"], false);
}

#[test]
fn test_typing_173_class_source_refuses_a_bad_class_or_property_name() {
    let ok = spec("A", evidence(1), None);
    assert!(class_source("X.Y", &[ok.clone()]).is_ok());
    for bad in ["", "X Y", "X) Kill ^Z", "1X", "X..Y", "X.Y\nSet a=1"] {
        assert!(
            class_source(bad, &[ok.clone()]).is_err(),
            "class {bad:?} must be refused"
        );
    }
    for bad in ["", "A B", "1A", "A;B", "A\"B", "%"] {
        let p = spec(bad, evidence(1), None);
        let err = class_source("X.Y", &[p]).expect_err(&format!("property {bad:?}"));
        assert!(err.contains("property name"), "{bad:?}: {err}");
    }
}

#[test]
fn test_typing_173_class_source_refuses_a_duplicate_property_ignoring_case() {
    let err = class_source(
        "X.Y",
        &[
            spec("Name", evidence(1), None),
            spec("name", evidence(1), None),
        ],
    )
    .unwrap_err();
    assert!(err.to_lowercase().contains("duplicate"), "{err}");
}

#[test]
fn test_typing_173_class_source_refuses_a_collation_that_is_not_a_collation_parameter() {
    let p = spec("A", evidence(1), Some("COLLATION = \"EXACT\") Kill ^Y //"));
    assert!(class_source("X.Y", &[p]).is_err());
    let p = spec("A", evidence(1), Some("MAXLEN = 5"));
    assert!(class_source("X.Y", &[p]).is_err());
}
