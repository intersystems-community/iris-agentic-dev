//! T062: `write::scratch_source` builds the scratch copy from the model (spec 172 FR-015a): the
//! class header with the same superclasses, the properties and indices the map names, the
//! keywords the map needs, `SqlTableName` set to the scratch name, and the emitted Storage block.
//! Nothing else from the class text: no method, query, XData, parameter, projection or include.

use iris_agentic_dev_core::tools::storage_map::tree::{emit_body, parse_body, split_class, Node};
use iris_agentic_dev_core::tools::storage_map::write::scratch_source;

const SCRATCH: &str = "IrisDevTmp.Sm172abcdef012345";

fn fixture(name: &str) -> String {
    let path = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures/storage172/classes")
        .join(format!("{name}.cls"));
    std::fs::read_to_string(&path).unwrap_or_else(|e| panic!("{}: {e}", path.display()))
}

/// The nodes of `text`'s first Storage block.
fn nodes_of(text: &str) -> Vec<Node> {
    let span = split_class(text)
        .expect("split")
        .into_iter()
        .next()
        .expect("a Storage block");
    parse_body(&text[span.body_start..span.body_end]).expect("parse")
}

/// `Member` with a method, a query, an XData block, a parameter, an include and a property the
/// map does not name, none of which may reach the scratch copy.
fn member_with_extras() -> String {
    let base = fixture("Member");
    let extras = "Parameter SECRET = \"pw\";\n\n\
        Property Unused As %String;\n\n\
        ClassMethod Leak() As %Status\n{\n Set ^IadLeak=1\n Property Fake As %String;\n Quit $$$OK\n}\n\n\
        Query ByCity(city As %String) As %SQLQuery\n{\nSELECT Name FROM IadSm172.Member WHERE City = :city\n}\n\n\
        XData Blob [ MimeType = application/json ]\n{\n{\"k\":1}\n}\n\n\
        Projection Pj As %Projection.Java;\n\n";
    let with = base.replacen(
        "Storage SQLStorage",
        &format!("{extras}Storage SQLStorage"),
        1,
    );
    format!("Include %occInclude\n\n{with}")
}

#[test]
fn test_storage_map_write_172_scratch_drops_everything_but_the_map_parts() {
    let real = member_with_extras();
    let nodes = nodes_of(&real);
    let out = scratch_source(&real, &nodes, SCRATCH).expect("scratch source");

    for gone in [
        "Leak",
        "^IadLeak",
        "Fake",
        "ByCity",
        "XData",
        "Blob",
        "SECRET",
        "Parameter",
        "Projection",
        "Include",
        "Unused",
        "IadSm172.Member",
    ] {
        assert!(
            !out.contains(gone),
            "{gone} reached the scratch copy:\n{out}"
        );
    }
    assert!(
        out.starts_with(&format!(
            "Class {SCRATCH} Extends %Persistent [ SqlTableName = Sm172abcdef012345, StorageStrategy = SQLStorage ]\n{{\n"
        )),
        "header:\n{out}"
    );
    for kept in [
        "Property Name As %String;",
        "Property City As %String;",
        "Property Age As %Integer;",
    ] {
        assert!(out.contains(kept), "{kept} missing:\n{out}");
    }
}

#[test]
fn test_storage_map_write_172_scratch_storage_is_emit_body() {
    let real = member_with_extras();
    // The model the caller proposes, not the stored one: the copy must carry it unchanged.
    let mut nodes = nodes_of(&real);
    nodes.retain(|n| n.tag != "SqlIdExpression");
    let out = scratch_source(&real, &nodes, SCRATCH).expect("scratch source");
    let spans = split_class(&out).expect("the scratch copy splits");
    assert_eq!(spans.len(), 1, "{out}");
    assert_eq!(spans[0].name, "SQLStorage");
    assert_eq!(
        &out[spans[0].body_start..spans[0].body_end],
        emit_body(&nodes),
        "the Storage body is not byte-identical to emit_body"
    );
    assert!(!out.contains("SqlIdExpression"), "{out}");
    assert!(out.ends_with("}\n"), "{out:?}");
}

#[test]
fn test_storage_map_write_172_scratch_keeps_idkey_and_map_indices() {
    let copy = fixture("Copy");
    let out = scratch_source(&copy, &nodes_of(&copy), SCRATCH).expect("Copy");
    assert!(
        out.contains("Index IDKEY On (BookId, Branch, CopyNo) [ IdKey, Unique ];"),
        "{out}"
    );
    for p in ["BookId", "Branch", "CopyNo"] {
        assert!(out.contains(&format!("Property {p} As")), "{p}:\n{out}");
    }

    let genre = fixture("Genre");
    let out = scratch_source(&genre, &nodes_of(&genre), SCRATCH).expect("Genre");
    assert!(
        out.contains("Index PopularBitmap On Popular [ Type = bitmap ];"),
        "{out}"
    );
    assert!(out.contains("Property Popular As %Boolean;"), "{out}");
}

#[test]
fn test_storage_map_write_172_scratch_replaces_the_table_name() {
    let real = fixture("Member").replacen(
        "[ StorageStrategy = SQLStorage ]",
        "[ SqlTableName = Members, StorageStrategy = SQLStorage, ClassType = persistent, CompileAfter = (A, B) ]",
        1,
    );
    let out = scratch_source(&real, &nodes_of(&real), SCRATCH).expect("scratch source");
    let header = out.lines().next().unwrap_or_default();
    assert_eq!(
        header,
        format!(
            "Class {SCRATCH} Extends %Persistent [ SqlTableName = Sm172abcdef012345, StorageStrategy = SQLStorage ]"
        )
    );
}

#[test]
fn test_storage_map_write_172_scratch_refuses_bad_inputs() {
    let real = fixture("Member");
    let nodes = nodes_of(&real);
    assert!(
        scratch_source(&real, &nodes, "IadSm172.Member").is_err(),
        "a scratch name outside IrisDevTmp"
    );
    assert!(
        scratch_source(&real, &nodes, "IrisDevTmp.Sm\"x").is_err(),
        "a scratch name that is not a class name"
    );
    assert!(
        scratch_source("Property X As %String;\n", &nodes, SCRATCH).is_err(),
        "text with no Class header"
    );
    let no_storage = "Class IadSm172.X Extends %Persistent\n{\n\nProperty X As %String;\n\n}\n";
    assert!(
        scratch_source(no_storage, &nodes, SCRATCH).is_err(),
        "a class with no Storage block"
    );
}

/// Default storage is read-only in `iris_storage_map` (FR-002), and IRIS refuses a second class on
/// its globals (#5564, measured), so there is no scratch copy of it.
#[test]
fn test_storage_map_write_172_scratch_refuses_default_storage() {
    let book = fixture("DefaultBook");
    let err = scratch_source(&book, &nodes_of(&book), SCRATCH).expect_err("default storage");
    assert!(err.contains("default storage"), "{err}");
}
