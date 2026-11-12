//! T063: `explain::explain` turns an IRIS compile error that names a storage-map error number
//! into plain words (spec 172 FR-017). An unknown number gives `None`.

use iris_agentic_dev_core::tools::storage_map::explain::{explain, EXPLAINED};

/// The numbers the task names (research R4/R5 and the compile errors map elements raise).
const NUMBERS: [u32; 24] = [
    5002, 5502, 5508, 5509, 5510, 5511, 5513, 5524, 5538, 5539, 5541, 5542, 5543, 5544, 5545, 5546,
    5547, 5548, 5549, 5583, 5585, 5596, 15511, 16023,
];

#[test]
fn test_storage_map_explain_172_every_number_has_text() {
    for n in NUMBERS {
        let got = explain(&format!("ERROR #{n}: whatever IRIS said"))
            .unwrap_or_else(|| panic!("#{n} has no explanation"));
        assert!(got.len() > 30, "#{n}: too short to help: {got}");
        assert!(
            !got.contains("%1"),
            "#{n}: an unfilled IRIS placeholder: {got}"
        );
    }
    let mut listed: Vec<u32> = EXPLAINED.iter().map(|(n, _)| *n).collect();
    listed.sort();
    assert_eq!(
        listed,
        NUMBERS.to_vec(),
        "EXPLAINED and the task list differ"
    );
}

#[test]
fn test_storage_map_explain_172_reads_the_number_from_real_error_text() {
    // Captured from the T061 live test.
    let real = "ERROR #5511: SQL Map type 'DATA' is invalid\r\n  > ERROR #5030: An error occurred \
                while compiling class 'IrisDevTmp.Sm172Chk0123'";
    let got = explain(real).expect("#5511");
    assert!(got.contains("data") && got.contains("index"), "{got}");
    assert!(
        got.to_lowercase().contains("case"),
        "#5511 is case-sensitive (R4): {got}"
    );
    // The number may come without a space or with "ERROR#".
    assert!(explain("ERROR#5596: Global name is missing").is_some());
}

#[test]
fn test_storage_map_explain_172_unknown_is_none() {
    assert_eq!(
        explain("ERROR #5030: An error occurred while compiling class 'X'"),
        None
    );
    assert_eq!(explain("ERROR #9999: nothing"), None);
    assert_eq!(explain("no number at all"), None);
    assert_eq!(explain("ERROR #55110: longer number"), None);
}

/// `check`'s `explanations`: one `{code, text}` per known number, in order, once each.
#[test]
fn test_storage_map_explain_172_explanations_for_an_error_list() {
    use iris_agentic_dev_core::tools::storage_map::explain::explanations;
    let errors = vec![
        "ERROR #5511: SQL Map type 'DATA' is invalid\r\n  > ERROR #5030: An error occurred"
            .to_string(),
        "ERROR #5030: An error occurred while compiling class 'X'".to_string(),
        "ERROR #5511: SQL Map type 'DATA' is invalid".to_string(),
        "ERROR #5596: Global name is missing for SQL map 'M' in table 'T'.".to_string(),
    ];
    let got = explanations(&errors);
    let codes: Vec<u64> = got.iter().filter_map(|e| e["code"].as_u64()).collect();
    assert_eq!(codes, vec![5511, 5596]);
    assert!(got
        .iter()
        .all(|e| e["text"].as_str().is_some_and(|t| t.len() > 30)));
    assert!(explanations(&["ERROR #5030: x".to_string()]).is_empty());
}
