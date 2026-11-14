//! Spec 172: the write path's pure refusals and records that no live test reaches: the scratch
//! copy keeps `Import` lines and a header with no keyword list (FR-015a), a read-back where IRIS
//! has an element in another shape (R9), a Storage block that does not parse in a class that
//! splits (FR-012a), the audit record of a put whose text iad cannot parse (FR-030c), an
//! `iris_doc` text that names an SQLMap outside any Storage block (FR-031), and the compile-time
//! gate on the scratch copy (FR-015).

use iris_agentic_dev_core::iris::connection::{DiscoverySource, IrisConnection};
use iris_agentic_dev_core::tools::storage_map::tree::{parse_body, split_class, Node};
use iris_agentic_dev_core::tools::storage_map::write::{
    doc_precheck, doc_write_record, gate_blocks, readback_diff, scratch_probe, scratch_source,
};
use serde_json::Value;

const SCRATCH: &str = "IrisDevTmp.Sm172abcdef012345";

fn member() -> String {
    let path = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures/storage172/classes/Member.cls");
    std::fs::read_to_string(&path).unwrap_or_else(|e| panic!("{}: {e}", path.display()))
}

fn nodes_of(text: &str) -> Vec<Node> {
    let span = split_class(text)
        .expect("split")
        .into_iter()
        .next()
        .expect("a Storage block");
    parse_body(&text[span.body_start..span.body_end]).expect("parse")
}

/// `Member` with its master map's closing tag gone: the class does not split.
fn unclosed_map() -> String {
    let text = member();
    assert!(text.contains("</SQLMap>"));
    text.replacen("</SQLMap>\n", "", 1)
}

/// `Member` with a Storage body over the parser's 1 MiB cap: the class splits (the cap is the
/// body parser's), the block does not parse.
fn oversized_block() -> String {
    let big = format!("<Description>{}</Description>\n", "x".repeat(1024 * 1024));
    member().replacen("<SqlIdExpression>", &format!("{big}<SqlIdExpression>"), 1)
}

#[test]
fn scratch_copy_keeps_import_lines_ahead_of_its_class_line() {
    let real = format!("Import IadSm172.Lib\n\n{}", member());
    let out = scratch_source(&real, &nodes_of(&real), SCRATCH).expect("scratch source");
    assert!(
        out.starts_with(&format!("Import IadSm172.Lib\n\nClass {SCRATCH}")),
        "{out}"
    );
}

#[test]
fn scratch_copy_of_a_header_without_keywords_keeps_its_superclass() {
    let real = member().replacen(" [ StorageStrategy = SQLStorage ]", "", 1);
    assert!(!real.lines().next().unwrap().contains('['), "{real}");
    let out = scratch_source(&real, &nodes_of(&real), SCRATCH).expect("scratch source");
    let header = out
        .lines()
        .find(|l| l.starts_with("Class "))
        .expect("a Class line");
    assert!(
        header.starts_with(&format!("Class {SCRATCH} Extends %Persistent")),
        "{header}"
    );
}

#[test]
fn readback_with_an_element_in_another_shape_is_unexplained() {
    let text = member();
    let stored = nodes_of(&text);
    let other = text.replacen(
        "<Global>^IadSm172Member</Global>",
        "<Global>\n<Piece>1</Piece>\n</Global>",
        1,
    );
    let d = readback_diff(&stored, &nodes_of(&other));
    assert!(
        d.unexplained
            .iter()
            .any(|u| u.contains("Global") && u.contains("sent as one shape, IRIS has another")),
        "{d:?}"
    );
}

#[test]
fn gate_blocks_refuses_a_block_it_cannot_parse_without_judging_it() {
    let mut judged = 0;
    let got = gate_blocks(&oversized_block(), |_| {
        judged += 1;
        None
    })
    .expect("a refusal");
    assert_eq!(got["error_code"], "STORAGE_MAP_PARSE_FAILED", "{got}");
    assert!(
        got["error"]
            .as_str()
            .unwrap_or("")
            .contains("cannot parse Storage SQLStorage"),
        "{got}"
    );
    assert_eq!(judged, 0, "a block iad cannot read was judged");
}

#[test]
fn doc_write_record_names_the_text_it_cannot_parse() {
    let before = member();
    let record = doc_write_record("IadSm172.Member", Some(&before), &unclosed_map());
    let unparsed: Vec<&str> = record["unparsed"]
        .as_array()
        .expect("an unparsed list")
        .iter()
        .filter_map(Value::as_str)
        .collect();
    assert_eq!(unparsed.len(), 1, "{record}");
    assert!(unparsed[0].starts_with("after: "), "{record}");
    assert_eq!(record["version_after"], "", "{record}");
    assert_ne!(record["version_before"], "", "{record}");
}

#[tokio::test]
async fn doc_precheck_refuses_an_sqlmap_outside_every_storage_block() {
    // Default storage, and an `<SQLMap` in an XData block after it: iad finds no map block, so
    // it refuses rather than let a map it never read through. Refused before any request, so the
    // connection is never used.
    let text = "Class IadSm172.Loose Extends %Persistent\n{\n\nProperty Name As %String;\n\n\
                Storage Default\n{\n<Data name=\"LooseDefaultData\">\n<Value name=\"1\">\n\
                <Value>Name</Value>\n</Value>\n</Data>\n<Type>%Storage.Persistent</Type>\n}\n\n\
                XData Notes\n{\n<notes><SQLMap name=\"x\"/></notes>\n}\n\n}\n";
    let iris = IrisConnection::new(
        "http://127.0.0.1:9",
        "USER",
        "nobody",
        "x",
        DiscoverySource::ExplicitFlag,
    );
    let client = reqwest::Client::new();
    let got = match doc_precheck(&iris, &client, "IadSm172.Loose.cls", text, "USER", true).await {
        Err(v) => v,
        Ok(_) => panic!("a text with an SQLMap outside a Storage block was let through"),
    };
    assert_eq!(got["error_code"], "STORAGE_MAP_PARSE_FAILED", "{got}");
    assert!(
        got["error"]
            .as_str()
            .unwrap_or("")
            .contains("no Storage block holding it"),
        "{got}"
    );
}

#[test]
fn scratch_probe_runs_the_compile_time_gate_on_the_copy() {
    // Outside a call the compile-time gate is on (fails closed), and a `##Expression` in map code
    // runs at compile time.
    let text = member().replacen(
        "<Expression>{ID}</Expression>",
        "<Expression>##Expression(1)</Expression>",
        1,
    );
    let got = scratch_probe(&text, &nodes_of(&text)).expect_err("the gate refuses it");
    assert_eq!(got["success"], false, "{got}");
    assert_ne!(got["error_code"], "STORAGE_MAP_INVALID", "{got}");
}
