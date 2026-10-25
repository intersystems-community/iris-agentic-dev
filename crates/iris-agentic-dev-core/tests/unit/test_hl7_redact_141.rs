//! Spec 141 R5 C19 and C22: HL7 v2 redaction holds across mixed line endings, and a body iad cannot
//! redact is not passed through when redaction is forced.

use iris_agentic_dev_core::tools::interop::{redact_hl7v2, redact_hl7v2_only};

const MSH: &str = "MSH|^~\\&|SendApp|SendFac|RecApp|RecFac|20260101||ADT^A01|1|P|2.5";
const PID1: &str = "PID|1||MRN1||Doe^Jane||19800101|F|||1 Main St||555-1111";
const PID2: &str = "PID|1||MRN2||Roe^Rick||19790202|M|||2 Oak Ave||555-2222";
const PID3: &str = "PID|1||MRN3||Poe^Edgar||19780303|M|||3 Elm Rd||555-3333";

/// C19: `\r`, `\n` and `\r\n` in one body. The redactor picked one separator for the whole body, so
/// everything after the first `\r` sat on one "line" and only the first PID was redacted.
#[test]
fn mixed_line_endings_redact_every_pid() {
    let body = format!("{MSH}\r{PID1}\n{PID2}\r\n{PID3}");
    let out = redact_hl7v2(&body);
    for name in ["Doe", "Roe", "Poe", "MRN1", "MRN2", "MRN3", "SendApp"] {
        assert!(!out.contains(name), "{name} survived redaction: {out:?}");
    }
    assert_eq!(
        out.matches("[REDACTED]").count(),
        1 + 3 * 6,
        "MSH-3 and PID-3, 5, 7, 8, 11, 13 of three segments (the fixture stops before PID-18): {out:?}"
    );
    // The separators are kept as found.
    let sep = |s: &str| -> Vec<char> { s.chars().filter(|c| *c == '\r' || *c == '\n').collect() };
    assert_eq!(sep(&out), sep(&body), "line endings changed: {out:?}");
}

#[test]
fn a_body_without_redactable_fields_comes_back_byte_identical() {
    let body = format!("{MSH}\r\nEVN|A01|20260101\n");
    let out = redact_hl7v2(&body);
    assert_eq!(out.matches("[REDACTED]").count(), 1, "{out:?}");
    assert_eq!(out.len(), body.len() - "SendApp".len() + "[REDACTED]".len());
    assert!(out.ends_with("EVN|A01|20260101\n"), "{out:?}");
}

#[test]
fn uniform_endings_still_work() {
    for sep in ["\r", "\n", "\r\n"] {
        let body = [MSH, PID1, PID2].join(sep);
        let out = redact_hl7v2(&body);
        assert!(
            !out.contains("Doe") && !out.contains("Roe"),
            "{sep:?}: {out:?}"
        );
    }
}

/// C22: forced redaction. HL7 v2 comes back redacted; anything else is `None`, so the caller refuses
/// it instead of returning it raw.
#[test]
fn redact_hl7v2_only_redacts_hl7_and_declines_everything_else() {
    let hl7 = format!("{MSH}\r{PID1}");
    let out = redact_hl7v2_only(&hl7).expect("an HL7 v2 body is redactable");
    assert!(!out.contains("Doe"), "{out:?}");
    for other in [
        "plain text with a name: Jane Doe",
        "<Patient><Name>Jane Doe</Name></Patient>",
        "{\"name\":\"Jane Doe\"}",
        "",
        "PID|1||MRN1||Doe^Jane",
    ] {
        assert_eq!(redact_hl7v2_only(other), None, "{other:?} must be refused");
    }
}
