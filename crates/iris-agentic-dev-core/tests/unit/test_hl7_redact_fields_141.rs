//! Spec 141 T195: HL7 v2 redaction masks the identifying fields of PID, NK1, GT1, IN1 and IN2, and
//! it does not fail open when a segment carries leading whitespace, an MLLP start byte (`\x0b`) or
//! a file separator (`\x1c`).
//!
//! The field positions below are written out here, independently of the list in the code, from the
//! HL7 v2.5 standard (field N of a segment is the Nth `|`-separated element after the segment
//! name). A change to the code's list that drops one of them fails this file.

use iris_agentic_dev_core::tools::interop::{redact_hl7v2, redact_hl7v2_only};

const MSH: &str = "MSH|^~\\&|SendApp141|SendFac|RecApp|RecFac|20260101||ADT^A01|1|P|2.5";

/// A segment of `count` fields where field N holds the token `<name>N~`.
fn segment(name: &str, count: usize) -> String {
    let mut line = name.to_string();
    for n in 1..=count {
        line.push_str(&format!("|{name}{n}~"));
    }
    line
}

fn token(name: &str, n: usize) -> String {
    format!("{name}{n}~")
}

/// Positions masked per segment, with the v2.5 field each one is. Anything not listed must come
/// back untouched (the set id, relationship, dates of coverage and the like carry no identity).
const MASKED: [(&str, usize, &[usize]); 5] = [
    (
        "PID",
        30,
        &[
            2,  // Patient ID (legacy; often holds the MRN)
            3,  // Patient Identifier List
            4,  // Alternate Patient ID
            5,  // Patient Name
            6,  // Mother's Maiden Name
            7,  // Date/Time of Birth
            8,  // Administrative Sex
            9,  // Patient Alias
            11, // Patient Address
            12, // County Code
            13, // Phone Number - Home
            14, // Phone Number - Business
            18, // Patient Account Number
            19, // SSN Number
            20, // Driver's License Number
        ],
    ),
    (
        "NK1",
        39,
        &[
            2,  // Name
            4,  // Address
            5,  // Phone Number
            6,  // Business Phone Number
            12, // Next of Kin Employee Number
            16, // Date/Time of Birth
            30, // Contact Person's Name
            31, // Contact Person's Telephone Number
            32, // Contact Person's Address
            33, // Next of Kin/Associated Party's Identifiers
            37, // Contact Person Social Security Number
        ],
    ),
    (
        "GT1",
        30,
        &[
            2,  // Guarantor Number
            3,  // Guarantor Name
            4,  // Guarantor Spouse Name
            5,  // Guarantor Address
            6,  // Guarantor Phone Number - Home
            7,  // Guarantor Phone Number - Business
            8,  // Guarantor Date/Time of Birth
            12, // Guarantor SSN
            16, // Guarantor Employer Name
            17, // Guarantor Employer Address
            18, // Guarantor Employer Phone Number
            19, // Guarantor Employee ID Number
            29, // Guarantor Employer ID Number
        ],
    ),
    (
        "IN1",
        49,
        &[
            10, // Insured's Group Employee ID
            16, // Name of Insured
            18, // Insured's Date of Birth
            19, // Insured's Address
            36, // Policy Number
            43, // Insured's Administrative Sex
            44, // Insured's Employer's Address
            49, // Insured's ID Number
        ],
    ),
    (
        "IN2",
        10,
        &[
            1, // Insured's Employee ID
            2, // Insured's Social Security Number
        ],
    ),
];

#[test]
fn each_segment_masks_exactly_its_listed_fields() {
    for (name, count, masked) in MASKED {
        let line = segment(name, count);
        let out = redact_hl7v2(&format!("{MSH}\r{line}"));
        let got: Vec<&str> = out
            .split('\r')
            .find(|l| l.starts_with(&format!("{name}|")))
            .unwrap_or_else(|| panic!("{name} segment missing from {out:?}"))
            .split('|')
            .collect();
        assert_eq!(got.len(), count + 1, "{name}: field count changed");
        for (n, field) in got.iter().enumerate().skip(1) {
            let want = if masked.contains(&n) {
                "[REDACTED]".to_string()
            } else {
                token(name, n)
            };
            assert_eq!(*field, want, "{name}-{n} in {out:?}");
        }
    }
}

/// The bytes that can sit in front of a segment name: nothing, blanks, MLLP start, file separator
/// and combinations.
const LEADS: [&str; 8] = ["", " ", "   ", "\t", "\x0b", "\x1c", "\x0b ", " \x1c"];

/// Every identifying token of every segment in `MASKED`, so a leak is named by its field.
fn secrets() -> Vec<String> {
    MASKED
        .iter()
        .flat_map(|(name, _, masked)| masked.iter().map(move |n| token(name, *n)))
        .collect()
}

fn body(lead: &str, sep: &str) -> String {
    let mut segments = vec![format!("{lead}{MSH}")];
    for (name, count, _) in MASKED {
        segments.push(format!("{lead}{}", segment(name, count)));
    }
    segments.join(sep)
}

#[test]
fn a_leading_byte_on_a_segment_does_not_stop_it_being_redacted() {
    for lead in LEADS {
        for sep in ["\r", "\n", "\r\n"] {
            let input = body(lead, sep);
            let out = redact_hl7v2(&input);
            for secret in secrets().iter().map(String::as_str).chain(["SendApp141"]) {
                assert!(
                    !out.contains(secret),
                    "lead {lead:?}, separator {sep:?}: {secret} survived: {out:?}"
                );
            }
            assert!(
                out.starts_with(lead) && out.matches(sep).count() == input.matches(sep).count(),
                "lead {lead:?}, separator {sep:?}: framing changed: {out:?}"
            );
        }
    }
}

#[test]
fn a_body_that_starts_with_a_leading_byte_is_still_hl7_for_forced_redaction() {
    for lead in LEADS {
        let input = body(lead, "\r");
        let out = redact_hl7v2_only(&input)
            .unwrap_or_else(|| panic!("lead {lead:?}: refused an HL7 v2 body"));
        assert!(!out.contains("PID5~"), "lead {lead:?}: {out:?}");
    }
}

/// A frame as it arrives off a wire: start byte, segments, end byte and a carriage return.
#[test]
fn a_whole_mllp_frame_is_redacted() {
    let input = format!("\x0b{}\r\x1c\r", body("", "\r"));
    let out = redact_hl7v2(&input);
    assert!(
        out.starts_with('\x0b') && out.ends_with("\x1c\r"),
        "{out:?}"
    );
    for secret in secrets() {
        assert!(!out.contains(&secret), "{secret} survived: {out:?}");
    }
}

/// A segment boundary the receiving parser would not honour is still a place the text can hide in
/// a model's eyes: the segment after a stray `\x0b` or `\x1c` is redacted as well.
#[test]
fn a_stray_frame_byte_between_segments_hides_nothing() {
    for stray in ["\x0b", "\x1c"] {
        let input = format!("{MSH}\r{}{stray}{}", segment("PID", 30), segment("NK1", 39));
        let out = redact_hl7v2(&input);
        for secret in ["PID5~", "NK12~", "NK14~"] {
            assert!(!out.contains(secret), "{stray:?}: {secret} in {out:?}");
        }
    }
}

#[test]
fn a_segment_name_padded_before_the_bar_is_matched() {
    let input = format!("{MSH}\rPID |1||MRN141||Doe141^Jane");
    let out = redact_hl7v2(&input);
    assert!(
        !out.contains("Doe141") && !out.contains("MRN141"),
        "{out:?}"
    );
}

/// Other segments are left alone; this pins what the flag does not cover.
#[test]
fn segments_outside_the_list_are_not_changed() {
    let pv1 = "PV1|1|I|W^389^1|||^^^^^^^^|ATTENDING^DOC|||SUR||||ADM|A0|";
    let obx = "OBX|1|ST|NAME^Patient name||Jane Doe141||||||F";
    let input = format!("{MSH}\r{pv1}\r{obx}");
    let out = redact_hl7v2(&input);
    assert!(
        out.ends_with(&format!("{pv1}\r{obx}")),
        "PV1 and OBX are not in the masked set: {out:?}"
    );
}

/// An empty field has no value to mask; marking it `[REDACTED]` would claim there was one.
#[test]
fn empty_fields_stay_empty() {
    let out = redact_hl7v2(&format!("{MSH}\rPID|1||MRN141||Doe141^Jane"));
    assert!(
        out.ends_with("PID|1||[REDACTED]||[REDACTED]"),
        "PID-2 and PID-4 are empty and stay so: {out:?}"
    );
}
