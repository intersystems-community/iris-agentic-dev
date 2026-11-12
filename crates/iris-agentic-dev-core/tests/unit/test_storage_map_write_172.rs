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
    // Every property is carried, mapped or not: the real compile refuses a property no data map
    // files (#5502, measured), so the copy must too, or check would pass a map the real class
    // rejects.
    for kept in [
        "Property Unused As %String;",
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

// ── T071: new class text and the read-back check ─────────────────────────────

mod t071 {
    use iris_agentic_dev_core::tools::storage_map::path::apply;
    use iris_agentic_dev_core::tools::storage_map::tree::{
        parse_body, split_class, Node, StorageSpan,
    };
    use iris_agentic_dev_core::tools::storage_map::write::{new_class_text, readback_diff};
    use serde_json::json;

    fn fixture(name: &str) -> String {
        let p = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("tests/fixtures/storage172/classes")
            .join(format!("{name}.cls"));
        std::fs::read_to_string(&p).unwrap_or_else(|e| panic!("{}: {e}", p.display()))
    }

    fn split(text: &str) -> (StorageSpan, Vec<Node>) {
        let span = split_class(text).unwrap().remove(0);
        let nodes = parse_body(&text[span.body_start..span.body_end]).unwrap();
        (span, nodes)
    }

    #[test]
    fn t071_no_change_gives_the_original_text_for_every_fixture() {
        let dir = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("tests/fixtures/storage172/classes");
        for e in std::fs::read_dir(dir).unwrap() {
            let p = e.unwrap().path();
            let text = std::fs::read_to_string(&p).unwrap();
            let (span, nodes) = split(&text);
            assert_eq!(
                new_class_text(&text, &span, &nodes),
                text,
                "{}",
                p.display()
            );
        }
    }

    #[test]
    fn t071_only_the_storage_body_changes() {
        let text = fixture("Member");
        let (span, mut nodes) = split(&text);
        apply(
            &mut nodes,
            &[json!({"op": "set", "path": "/maps/Master/data/Age/piece", "value": "4"})],
        )
        .unwrap();
        let out = new_class_text(&text, &span, &nodes);
        let a: Vec<&str> = text.lines().collect();
        let b: Vec<&str> = out.lines().collect();
        assert_eq!(a.len(), b.len());
        let diff: Vec<(&str, &str)> = a
            .iter()
            .zip(&b)
            .filter(|(x, y)| x != y)
            .map(|(x, y)| (*x, *y))
            .collect();
        assert_eq!(diff, vec![("<Piece>3</Piece>", "<Piece>4</Piece>")]);
    }

    /// IRIS writes the storage Description twice: as CDATA in the block and as `///` lines
    /// above `Storage <name>` (R3). A changed Description changes both.
    #[test]
    fn t071_a_changed_description_rewrites_its_comment_lines() {
        let text = fixture("Notes");
        let (span, mut nodes) = split(&text);
        apply(
            &mut nodes,
            &[json!({"op": "set", "path": "/keywords/Description", "value": "One note per node."})],
        )
        .unwrap();
        let out = new_class_text(&text, &span, &nodes);
        assert!(
            out.contains("/// One note per node.\nStorage SQLStorage\n{\n"),
            "{out}"
        );
        assert!(!out.contains("/// Library notes"), "{out}");
        assert!(
            out.contains("<![CDATA[One note per node.]]></Description>"),
            "{out}"
        );

        apply(
            &mut nodes,
            &[json!({"op": "remove", "path": "/keywords/Description"})],
        )
        .unwrap();
        let out = new_class_text(&text, &span, &nodes);
        assert!(!out.contains("///"), "{out}");
        assert!(out.contains("\n\nStorage SQLStorage\n{\n"), "{out}");
    }

    fn member_nodes(from: &str, to: &str) -> Vec<Node> {
        let text = fixture("Member");
        assert!(text.contains(from), "{from}");
        split(&text.replacen(from, to, 1)).1
    }

    #[test]
    fn t071_readback_diff() {
        let stored = split(&fixture("Member")).1;
        let same = readback_diff(&stored, &stored);
        assert!(
            same.dropped.is_empty() && same.unexplained.is_empty(),
            "{same:?}"
        );

        // IRIS drops an element it does not know (R3.6): reported as dropped.
        let sent = member_nodes(
            "<Global>^IadSm172Member</Global>",
            "<Global>^IadSm172Member</Global>\n<Wibble>x</Wibble>",
        );
        let d = readback_diff(&sent, &stored);
        assert_eq!(d.dropped.len(), 1, "{d:?}");
        assert!(d.dropped[0].contains("Wibble"), "{d:?}");
        assert!(d.unexplained.is_empty(), "{d:?}");

        // BlockCount 0 is dropped by IRIS (R3.4) and is no difference at all.
        let sent = member_nodes(
            "<Type>data</Type>",
            "<Type>data</Type>\n<BlockCount>0</BlockCount>",
        );
        let d = readback_diff(&sent, &stored);
        assert!(d.dropped.is_empty() && d.unexplained.is_empty(), "{d:?}");

        // A value IRIS changed, and an element it added, are unexplained.
        let changed = member_nodes("<Piece>3</Piece>", "<Piece>4</Piece>");
        let d = readback_diff(&changed, &stored);
        assert_eq!(d.unexplained.len(), 1, "{d:?}");
        let extra = member_nodes(
            "<Type>data</Type>",
            "<Type>data</Type>\n<Structure>x</Structure>",
        );
        assert!(!readback_diff(&stored, &extra).unexplained.is_empty());

        // Children out of IRIS's canonical order are unexplained (the canonical form is a fixed
        // point, R3.8).
        let mut reordered = stored.clone();
        reordered.reverse();
        let d = readback_diff(&stored, &reordered);
        assert!(!d.unexplained.is_empty(), "{d:?}");
    }
}
