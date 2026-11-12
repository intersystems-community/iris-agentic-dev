//! T070: `validate` holds a tree to data-model.md "Validation" (FR-014) before any write: one
//! failing case per rule, and every editable fixture passes.

use iris_agentic_dev_core::tools::storage_map::tree::{parse_body, split_class, Node};
use iris_agentic_dev_core::tools::storage_map::validate::{validate, validate_change, Invalid};

fn fixture_text(name: &str) -> String {
    let p = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures/storage172/classes")
        .join(format!("{name}.cls"));
    std::fs::read_to_string(&p).unwrap_or_else(|e| panic!("{}: {e}", p.display()))
}

fn nodes_of(text: &str) -> Vec<Node> {
    let span = split_class(text).unwrap().remove(0);
    parse_body(&text[span.body_start..span.body_end]).unwrap()
}

/// `Member` with `from` replaced by `to` in its Storage block.
fn member_with(from: &str, to: &str) -> Vec<Node> {
    let text = fixture_text("Member");
    assert!(text.contains(from), "Member has no {from:?}");
    nodes_of(&text.replacen(from, to, 1))
}

fn rules(r: Result<(), Vec<Invalid>>) -> Vec<(String, String, &'static str)> {
    r.err()
        .unwrap_or_default()
        .into_iter()
        .map(|i| (i.rule, i.path, i.code))
        .collect()
}

#[test]
fn test_storage_map_validate_172_every_editable_fixture_passes() {
    let dir = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures/storage172/classes");
    let mut n = 0;
    for e in std::fs::read_dir(dir).unwrap() {
        let p = e.unwrap().path();
        let name = p.file_stem().unwrap().to_string_lossy().to_string();
        let got = rules(validate(&nodes_of(&std::fs::read_to_string(&p).unwrap())));
        if name == "DefaultBook" {
            assert!(
                got.iter()
                    .any(|(rule, _, code)| rule == "read_only" && *code == "STORAGE_MAP_READ_ONLY"),
                "DefaultBook: {got:?}"
            );
        } else {
            assert!(got.is_empty(), "{name}: {got:?}");
            n += 1;
        }
    }
    assert!(n >= 14, "{n} fixtures");
}

#[test]
fn test_storage_map_validate_172_one_failure_per_rule() {
    let cases: Vec<(Vec<Node>, &str, &str)> = vec![
        (
            member_with("<Subscript name=\"1\">", "<Subscript name=\"a\">"),
            "integer_name",
            "/maps/Master/subscripts/a",
        ),
        (
            member_with("<Type>data</Type>", "<Type>DATA</Type>"),
            "map_type",
            "/maps/Master/type",
        ),
        (
            member_with(
                "<Expression>{ID}</Expression>",
                "<Expression>{ID}</Expression>\n<AccessType>Wibble</AccessType>",
            ),
            "access_type",
            "/maps/Master/subscripts/1/access_type",
        ),
        (
            member_with(
                "<Global>^IadSm172Member</Global>",
                "<Global>^IadSm172Member</Global>\n<Wibble>x</Wibble>",
            ),
            "unknown_element",
            "/maps/Master/extra/Wibble",
        ),
        (
            member_with(
                "<Global>^IadSm172Member</Global>",
                "<Global>^|\"USER\"|IadSm172Member</Global>",
            ),
            "extended_reference",
            "/maps/Master/global",
        ),
        (
            member_with(
                "<Global>^IadSm172Member</Global>",
                "<Global>^[\"USER\"]IadSm172Member</Global>",
            ),
            "extended_reference",
            "/maps/Master/global",
        ),
        (
            member_with("<Piece>3</Piece>", "<Piece></Piece>"),
            "empty_leaf",
            "/maps/Master/data/Age/piece",
        ),
        (
            member_with("<Type>data</Type>", "<Type>columnar</Type>"),
            "read_only",
            "/maps/Master/type",
        ),
        (
            member_with(
                "<Type>%Storage.SQL</Type>",
                "<Type>%Storage.Persistent</Type>",
            ),
            "read_only",
            "/keywords/Type",
        ),
    ];
    for (nodes, rule, path) in cases {
        let got = rules(validate(&nodes));
        assert!(
            got.iter().any(|(r, p, _)| r == rule && p == path),
            "{rule} at {path}: got {got:?}"
        );
        let code = got.iter().find(|(r, _, _)| r == rule).unwrap().2;
        let want = if rule == "read_only" {
            "STORAGE_MAP_READ_ONLY"
        } else {
            "STORAGE_MAP_INVALID"
        };
        assert_eq!(code, want, "{rule}");
    }
}

/// Access types are not case-sensitive (R4): `sub`, `PIECE` pass; the spelling is kept.
#[test]
fn test_storage_map_validate_172_access_type_ignores_case() {
    for a in ["sub", "PIECE", "list", "Global", "other"] {
        let nodes = member_with(
            "<Expression>{ID}</Expression>",
            &format!("<Expression>{{ID}}</Expression>\n<AccessType>{a}</AccessType>"),
        );
        assert!(validate(&nodes).is_ok(), "{a}");
    }
}

/// An element iad does not model may stay where IRIS already had it (it is reported as dropped
/// on save elsewhere); only one the write adds is refused.
#[test]
fn test_storage_map_validate_172_unknown_element_only_when_added() {
    let with = member_with(
        "<Global>^IadSm172Member</Global>",
        "<Global>^IadSm172Member</Global>\n<Wibble>x</Wibble>",
    );
    let without = nodes_of(&fixture_text("Member"));
    assert!(
        validate_change(&with, &with).is_ok(),
        "it was already there"
    );
    let got = rules(validate_change(&without, &with));
    assert!(
        got.iter().any(|(r, _, _)| r == "unknown_element"),
        "added: {got:?}"
    );
    // The other rules still apply to what was already there.
    let bad = member_with("<Type>data</Type>", "<Type>DATA</Type>");
    assert!(validate_change(&bad, &bad).is_err());
}
