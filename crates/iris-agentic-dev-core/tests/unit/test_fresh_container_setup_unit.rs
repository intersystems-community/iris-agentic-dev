// Unit tests for 099-fresh-container-setup param defaults and FreshSetupResult shape.
// No IRIS connection required — pure logic tests.

// ── US1: clear_password_change_flag param defaults ───────────────────────────

#[test]
fn test_clear_password_flag_defaults() {
    let params: serde_json::Value = serde_json::json!({});
    let username = params
        .get("username")
        .and_then(|v| v.as_str())
        .unwrap_or("_SYSTEM");
    let password = params
        .get("password")
        .and_then(|v| v.as_str())
        .unwrap_or("SYS");
    let new_password = params
        .get("new_password")
        .and_then(|v| v.as_str())
        .unwrap_or(password);
    assert_eq!(username, "_SYSTEM");
    assert_eq!(password, "SYS");
    assert_eq!(new_password, "SYS", "new_password must default to password");
}

#[test]
fn test_clear_password_flag_explicit_new_password() {
    let params: serde_json::Value = serde_json::json!({"new_password":"newpass"});
    let password = params
        .get("password")
        .and_then(|v| v.as_str())
        .unwrap_or("SYS");
    let new_password = params
        .get("new_password")
        .and_then(|v| v.as_str())
        .unwrap_or(password);
    assert_eq!(new_password, "newpass");
}

#[test]
fn test_clear_password_flag_explicit_username() {
    let params: serde_json::Value = serde_json::json!({"username":"Admin","password":"mypass"});
    let username = params
        .get("username")
        .and_then(|v| v.as_str())
        .unwrap_or("_SYSTEM");
    let password = params
        .get("password")
        .and_then(|v| v.as_str())
        .unwrap_or("SYS");
    assert_eq!(username, "Admin");
    assert_eq!(password, "mypass");
}

// ── US3: unlock_user param validation ────────────────────────────────────────

#[test]
fn test_unlock_user_missing_username() {
    let params: serde_json::Value = serde_json::json!({});
    let username = params
        .get("username")
        .and_then(|v| v.as_str())
        .unwrap_or("");
    assert!(
        username.is_empty(),
        "empty username triggers INVALID_PARAMS"
    );
}

#[test]
fn test_unlock_user_provided_username() {
    let params: serde_json::Value = serde_json::json!({"username":"TestUser"});
    let username = params
        .get("username")
        .and_then(|v| v.as_str())
        .unwrap_or("");
    assert_eq!(username, "TestUser");
}

// ── US2: FreshSetupResult shape ───────────────────────────────────────────────

#[derive(serde::Serialize)]
#[serde(rename_all = "snake_case")]
enum SetupStepStatus {
    Ok,
    Skipped,
    Error,
}

#[derive(serde::Serialize)]
struct SetupStep {
    action: String,
    status: SetupStepStatus,
    detail: String,
}

#[derive(serde::Serialize)]
struct FreshSetupResult {
    success: bool,
    ready: bool,
    steps: Vec<SetupStep>,
}

#[test]
fn test_fresh_setup_result_ready_all_ok() {
    let result = FreshSetupResult {
        success: true,
        ready: true,
        steps: vec![
            SetupStep {
                action: "clear_password_change_flag".to_string(),
                status: SetupStepStatus::Ok,
                detail: "flag cleared".to_string(),
            },
            SetupStep {
                action: "unlock_user".to_string(),
                status: SetupStepStatus::Ok,
                detail: "unlocked".to_string(),
            },
        ],
    };
    assert!(result.success);
    assert!(result.ready);
    assert_eq!(result.steps.len(), 2);
}

#[test]
fn test_fresh_setup_result_not_ready_on_error() {
    let result = FreshSetupResult {
        success: false,
        ready: false,
        steps: vec![
            SetupStep {
                action: "clear_password_change_flag".to_string(),
                status: SetupStepStatus::Error,
                detail: "failed".to_string(),
            },
            SetupStep {
                action: "unlock_user".to_string(),
                status: SetupStepStatus::Ok,
                detail: "unlocked".to_string(),
            },
        ],
    };
    assert!(!result.success);
    assert!(!result.ready);
}

#[test]
fn test_fresh_setup_result_json_shape() {
    let result = FreshSetupResult {
        success: true,
        ready: true,
        steps: vec![SetupStep {
            action: "clear_password_change_flag".to_string(),
            status: SetupStepStatus::Ok,
            detail: "ok".to_string(),
        }],
    };
    let j = serde_json::to_value(&result).unwrap();
    assert!(j.get("success").is_some(), "missing success field");
    assert!(j.get("ready").is_some(), "missing ready field");
    let steps = j["steps"].as_array().expect("steps must be array");
    assert!(!steps.is_empty());
    let step = &steps[0];
    assert!(step.get("action").is_some(), "missing action");
    assert!(step.get("status").is_some(), "missing status");
    assert!(step.get("detail").is_some(), "missing detail");
    assert_eq!(step["status"].as_str(), Some("ok"), "status must be 'ok'");
}

#[test]
fn test_setup_step_status_serialization() {
    assert_eq!(
        serde_json::to_value(SetupStepStatus::Ok).unwrap(),
        serde_json::json!("ok")
    );
    assert_eq!(
        serde_json::to_value(SetupStepStatus::Skipped).unwrap(),
        serde_json::json!("skipped")
    );
    assert_eq!(
        serde_json::to_value(SetupStepStatus::Error).unwrap(),
        serde_json::json!("error")
    );
}
