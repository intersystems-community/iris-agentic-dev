//! Spec 141 R5 C22 and FR-030c: bulk-data tools under a server `redact` policy.
//!
//! Under `redact`, `iris_message_body` and `stream_inspect` return HL7 v2 redacted and refuse
//! anything else with `BODY_NOT_REDACTABLE`. `stream_inspect` answers a refused body with the
//! stream's class, size and line count. `journal_search` stays refused: a journal record's value is
//! not a message, so iad has nothing to redact it with.

use iris_agentic_dev_core::iris::workspace_config::DataPolicy;
use iris_agentic_dev_core::policy::data_policy_gate::{
    check_bulk_phi_gate, BULK_PHI_TOOLS, HANDLER_ENFORCED_TOOLS, REDACTING_TOOLS,
};
use iris_agentic_dev_core::tools::interop::{
    apply_body_policy, stream_inspect_outcome, BodyRefusal, StreamFacts,
};
use iris_agentic_dev_core::tools::write_gate::CLASSIFICATION;

const HL7: &str = "MSH|^~\\&|SendApp|F|R|F|20260101||ADT^A01|1|P|2.5\rPID|1||MRN1||Doe^Jane||19800101|F|||1 Main St||555-1111";
const PLAIN: &str = "Jane Doe, born 1980-01-01\nMRN1";

#[test]
fn redact_admits_message_body_with_forced_redaction() {
    assert!(
        check_bulk_phi_gate("iris_message_body", &DataPolicy::Redact, "prod").is_none(),
        "a server on redact lets iris_message_body through; the handler redacts"
    );
    assert!(check_bulk_phi_gate("iris_message_body", &DataPolicy::Allow, "prod").is_none());
    let blocked = check_bulk_phi_gate("iris_message_body", &DataPolicy::Block, "prod")
        .expect("block still refuses");
    assert_eq!(blocked["error_code"], "DATA_POLICY_BLOCKED");
}

#[test]
fn journal_search_stays_refused_under_redact() {
    let refusal = check_bulk_phi_gate("journal_search", &DataPolicy::Redact, "prod")
        .expect("journal_search has nothing to redact with");
    assert_eq!(refusal["error_code"], "DATA_POLICY_BLOCKED");
    // The dispatcher route is the same tool.
    assert!(check_bulk_phi_gate("journal_search", &DataPolicy::Block, "prod").is_some());
}

/// A typo in a gate list fails open: the tool is not found in it and the call goes through. Every
/// name in the two sub-lists is a registered tool and a bulk-data tool.
#[test]
fn the_redacting_and_handler_enforced_lists_name_registered_bulk_tools() {
    assert!(!REDACTING_TOOLS.is_empty() && !HANDLER_ENFORCED_TOOLS.is_empty());
    for t in REDACTING_TOOLS.iter().chain(HANDLER_ENFORCED_TOOLS) {
        assert!(
            CLASSIFICATION.iter().any(|e| e.tool == *t),
            "{t} is not a registered tool"
        );
        assert!(BULK_PHI_TOOLS.contains(t), "{t} is not a bulk-data tool");
    }
}

#[test]
fn stream_inspect_is_a_bulk_data_tool_registered_in_the_gate_table() {
    assert!(BULK_PHI_TOOLS.contains(&"stream_inspect"));
    for t in BULK_PHI_TOOLS {
        assert!(
            CLASSIFICATION.iter().any(|e| e.tool == *t),
            "{t} is not a registered tool; a typo in this list fails open"
        );
    }
}

/// `stream_inspect` answers a refused body with metadata, so under `block` the gate leaves the
/// refusal to the handler instead of answering with nothing.
#[test]
fn stream_inspect_reaches_its_handler_under_every_policy() {
    for p in [DataPolicy::Block, DataPolicy::Redact, DataPolicy::Allow] {
        assert!(
            check_bulk_phi_gate("stream_inspect", &p, "prod").is_none(),
            "{p:?}"
        );
    }
}

#[test]
fn hl7_body_is_redacted_under_redact_and_raw_under_allow() {
    let redacted = apply_body_policy(&DataPolicy::Redact, HL7).unwrap();
    assert!(
        !redacted.contains("Doe") && !redacted.contains("MRN1"),
        "{redacted:?}"
    );
    assert_eq!(apply_body_policy(&DataPolicy::Allow, HL7).unwrap(), HL7);
    assert_eq!(
        apply_body_policy(&DataPolicy::Block, HL7),
        Err(BodyRefusal::Blocked)
    );
}

#[test]
fn non_hl7_body_refused_under_redact() {
    for body in [PLAIN, "<Patient><Name>Jane Doe</Name></Patient>", ""] {
        assert_eq!(
            apply_body_policy(&DataPolicy::Redact, body),
            Err(BodyRefusal::NotRedactable),
            "{body:?} would come back raw"
        );
        assert_eq!(
            apply_body_policy(&DataPolicy::Allow, body).as_deref(),
            Ok(body)
        );
    }
}

fn facts<'a>(content: &'a str) -> StreamFacts<'a> {
    StreamFacts {
        oid: "42",
        class: "%Stream.GlobalCharacter",
        size: content.len() as i64,
        content,
        truncated: false,
        max_chars: 20_000,
    }
}

#[test]
fn stream_under_block_returns_metadata_and_refuses_the_content() {
    let refusal = stream_inspect_outcome(&DataPolicy::Block, &facts(HL7)).unwrap_err();
    assert_eq!(refusal["error_code"], "DATA_POLICY_BLOCKED");
    assert_eq!(refusal["class"], "%Stream.GlobalCharacter");
    assert_eq!(refusal["size"], HL7.len() as i64);
    assert_eq!(refusal["line_count"], 2);
    assert_eq!(refusal["oid"], "42");
    assert!(refusal.get("content").is_none(), "{refusal}");
    assert!(!refusal.to_string().contains("Doe"), "{refusal}");
}

#[test]
fn stream_under_redact_redacts_hl7_and_refuses_the_rest() {
    let ok = stream_inspect_outcome(&DataPolicy::Redact, &facts(HL7)).unwrap();
    assert_eq!(ok["success"], true);
    assert!(!ok["content"].as_str().unwrap().contains("Doe"), "{ok}");
    assert_eq!(ok["redacted"], true);

    let refusal = stream_inspect_outcome(&DataPolicy::Redact, &facts(PLAIN)).unwrap_err();
    assert_eq!(refusal["error_code"], "BODY_NOT_REDACTABLE");
    assert_eq!(refusal["line_count"], 2);
    assert_eq!(refusal["size"], PLAIN.len() as i64);
    assert!(!refusal.to_string().contains("Jane"), "{refusal}");
}

#[test]
fn stream_under_allow_returns_the_content() {
    let ok = stream_inspect_outcome(&DataPolicy::Allow, &facts(PLAIN)).unwrap();
    assert_eq!(ok["content"], PLAIN);
    assert_eq!(ok["class"], "%Stream.GlobalCharacter");
    assert_eq!(ok["line_count"], 2);
}

#[test]
fn line_count_reads_every_separator() {
    let count = |s: &str| {
        stream_inspect_outcome(&DataPolicy::Allow, &facts(s)).unwrap()["line_count"].as_u64()
    };
    assert_eq!(count(""), Some(0));
    assert_eq!(count("a"), Some(1));
    assert_eq!(count("a\rb\nc\r\nd"), Some(4));
    assert_eq!(count("a\n"), Some(1));
}
