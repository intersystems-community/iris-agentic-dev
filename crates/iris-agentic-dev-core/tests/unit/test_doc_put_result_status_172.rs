// Test for iris_doc PUT result.status error detection
// Spec: when Atelier returns HTTP 200 with empty status.errors but error text in result.status,
// the tool must detect and report it as an error.

#[test]
fn test_check_put_result_status_empty_errors_with_status_error() {
    // Simulate a PUT response with empty status.errors but error text in result.status.
    // This is the bug case: IRIS 2026.2 can return 200, status.errors=[], and error in result.status.
    let put_body: serde_json::Value = serde_json::json!({
        "status": {
            "errors": []
        },
        "result": {
            "status": "ERROR #16023: TNode ... not found"
        }
    });

    // The helper should detect the error in result.status
    let result = check_put_result_status(&put_body);
    assert!(result.is_err(), "should detect error in result.status");
    let err = result.unwrap_err();
    assert!(
        err.contains("ERROR #16023"),
        "error message should include the IRIS error text"
    );
}

#[test]
fn test_check_put_result_status_with_status_errors() {
    // Traditional case: error in status.errors (non-empty array)
    let put_body: serde_json::Value = serde_json::json!({
        "status": {
            "errors": [
                {
                    "error": "ERROR #5001: Some error"
                }
            ]
        },
        "result": {
            "status": ""
        }
    });

    let result = check_put_result_status(&put_body);
    assert!(result.is_err(), "should detect error in status.errors");
    let err = result.unwrap_err();
    assert!(
        err.contains("ERROR #5001"),
        "error message should include status.errors text"
    );
}

#[test]
fn test_check_put_result_status_success_empty_errors() {
    // Success case: empty status.errors and no error in result.status
    let put_body: serde_json::Value = serde_json::json!({
        "status": {
            "errors": []
        },
        "result": {
            "status": ""
        }
    });

    let result = check_put_result_status(&put_body);
    assert!(
        result.is_ok(),
        "should succeed with empty errors and empty result.status"
    );
}

#[test]
fn test_check_put_result_status_success_result_status_absent() {
    // Success case: result.status is absent
    let put_body: serde_json::Value = serde_json::json!({
        "status": {
            "errors": []
        }
    });

    let result = check_put_result_status(&put_body);
    assert!(
        result.is_ok(),
        "should succeed when result.status is absent"
    );
}

#[test]
fn test_check_put_result_status_success_non_error_status() {
    // Success case: result.status exists but doesn't start with ERROR
    let put_body: serde_json::Value = serde_json::json!({
        "status": {
            "errors": []
        },
        "result": {
            "status": "OK"
        }
    });

    let result = check_put_result_status(&put_body);
    assert!(
        result.is_ok(),
        "should succeed when result.status doesn't start with ERROR"
    );
}

#[test]
fn test_check_put_result_status_error_with_leading_whitespace() {
    // Error in result.status with leading whitespace before ERROR
    let put_body: serde_json::Value = serde_json::json!({
        "status": {
            "errors": []
        },
        "result": {
            "status": "  ERROR #16023: Some error"
        }
    });

    let result = check_put_result_status(&put_body);
    assert!(
        result.is_err(),
        "should detect error with leading whitespace"
    );
    let err = result.unwrap_err();
    assert!(
        err.contains("ERROR #16023"),
        "error message should preserve the error text"
    );
}

// Helper function that matches the implementation
fn check_put_result_status(put_body: &serde_json::Value) -> Result<(), String> {
    // First check status.errors (traditional case)
    if let Some(errs) = put_body["status"]["errors"].as_array() {
        if !errs.is_empty() {
            let msg = errs[0]["error"]
                .as_str()
                .unwrap_or("Document upload failed");
            return Err(msg.to_string());
        }
    }

    // Then check result.status for error text (new case, matching delete_outcome pattern)
    match put_body["result"]["status"].as_str() {
        Some(text) if text.trim_start().starts_with("ERROR") => Err(text.trim().to_string()),
        _ => Ok(()),
    }
}
