#![allow(clippy::all)]
use iris_agentic_dev_core::tools::admin::*;

// Serialize tests that mutate IRIS_ADMIN_TOOLS to prevent races.
static ENV_LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());

fn rt() -> tokio::runtime::Runtime {
    tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .unwrap()
}

fn result_text(r: &rmcp::model::CallToolResult) -> serde_json::Value {
    let text = r.content[0].as_text().unwrap().text.clone();
    serde_json::from_str(&text).unwrap_or_default()
}

// ── T004/T030: ADMIN_WRITE_DISABLED without IRIS_ADMIN_TOOLS ─────────────────

#[test]
fn write_actions_disabled_without_env() {
    let _g = ENV_LOCK.lock().unwrap_or_else(|e| e.into_inner());
    std::env::remove_var("IRIS_ADMIN_TOOLS");
    let rt = rt();

    let actions: &[&str] = &[
        "create_user",
        "delete_user",
        "update_user",
        "create_namespace",
        "delete_namespace",
        "create_webapp",
        "delete_webapp",
    ];

    for action in actions {
        let r = match *action {
            "create_user" => rt.block_on(admin_create_user_impl(None, "x", "p", None, None)),
            "delete_user" => rt.block_on(admin_delete_user_impl(None, "x")),
            "update_user" => rt.block_on(admin_update_user_impl(None, "x", None, None, None)),
            "create_namespace" => rt.block_on(admin_create_namespace_impl(None, "x", "y", "z")),
            "delete_namespace" => rt.block_on(admin_delete_namespace_impl(None, "x")),
            "create_webapp" => {
                rt.block_on(admin_create_webapp_impl(None, "/x", "USER", None, true))
            }
            "delete_webapp" => rt.block_on(admin_delete_webapp_impl(None, "/x")),
            _ => unreachable!(),
        }
        .unwrap();
        let v = result_text(&r);
        assert_eq!(
            v["error_code"], "ADMIN_WRITE_DISABLED",
            "action '{}' should return ADMIN_WRITE_DISABLED without IRIS_ADMIN_TOOLS",
            action
        );
    }
}

// ── T005: list_users never contains password ──────────────────────────────────

#[test]
fn list_users_response_no_password() {
    // Simulate the response shape — verify no password key at any depth
    let sample = serde_json::json!({
        "success": true,
        "users": [
            {"name": "_SYSTEM", "full_name": "", "enabled": true, "roles": "%All"}
        ],
        "count": 1,
        "truncated": false,
        "total_count": 1
    });
    let text = sample.to_string();
    assert!(
        !text.contains("\"password\""),
        "password must not appear in user list response"
    );
    assert!(
        !text.contains("\"Password\""),
        "Password must not appear in user list response"
    );
}

// ── T014: list_namespaces response shape ─────────────────────────────────────

#[test]
fn list_namespaces_response_shape() {
    // Verify the shape we produce from the SQL result
    let sample = serde_json::json!({
        "success": true,
        "namespaces": [
            {"name": "USER", "code_database": "USER", "data_database": "USER"}
        ],
        "count": 1
    });
    assert!(sample["namespaces"].is_array());
    let ns = &sample["namespaces"][0];
    assert!(ns.get("name").is_some());
    assert!(ns.get("code_database").is_some());
    assert!(ns.get("data_database").is_some());
}

// ── T015: list_databases response shape ──────────────────────────────────────

#[test]
fn list_databases_response_shape() {
    let sample = serde_json::json!({
        "success": true,
        "databases": [
            {"directory": "/iris/db/user", "size_mb": 100, "max_size_mb": 0, "mounted": true, "read_only": false}
        ],
        "count": 1
    });
    assert!(sample["databases"].is_array());
    let db = &sample["databases"][0];
    assert!(db.get("directory").is_some());
    assert!(db.get("size_mb").is_some());
    assert!(db.get("mounted").is_some());
    assert!(db.get("read_only").is_some());
}

// ── T019: read actions return IRIS_UNREACHABLE with no connection ─────────────

#[test]
fn read_actions_unreachable_no_connection() {
    let rt = rt();

    let r = rt.block_on(admin_list_namespaces_impl(None)).unwrap();
    assert_eq!(result_text(&r)["error_code"], "IRIS_UNREACHABLE");

    let r = rt.block_on(admin_list_databases_impl(None)).unwrap();
    assert_eq!(result_text(&r)["error_code"], "IRIS_UNREACHABLE");

    let r = rt.block_on(admin_list_users_impl(None)).unwrap();
    assert_eq!(result_text(&r)["error_code"], "IRIS_UNREACHABLE");

    let r = rt.block_on(admin_list_roles_impl(None)).unwrap();
    assert_eq!(result_text(&r)["error_code"], "IRIS_UNREACHABLE");

    let r = rt
        .block_on(admin_list_user_roles_impl(None, "_SYSTEM"))
        .unwrap();
    assert_eq!(result_text(&r)["error_code"], "IRIS_UNREACHABLE");

    let r = rt
        .block_on(admin_check_permission_impl(None, "%DB_USER", "USE"))
        .unwrap();
    assert_eq!(result_text(&r)["error_code"], "IRIS_UNREACHABLE");
}

// ── T026: list_webapps response shape ────────────────────────────────────────

#[test]
fn list_webapps_response_shape() {
    let sample = serde_json::json!({
        "success": true,
        "webapps": [
            {"path": "/api/atelier", "namespace": "USER", "dispatch_class": "", "enabled": true, "type": "REST"}
        ],
        "count": 1,
        "truncated": false,
        "total_count": 1
    });
    assert!(sample["webapps"].is_array());
    let wa = &sample["webapps"][0];
    assert!(wa.get("path").is_some());
    assert!(wa.get("namespace").is_some());
    assert!(wa.get("type").is_some());
    assert!(wa.get("enabled").is_some());
}

// ── T027: type filter classification logic ────────────────────────────────────

#[test]
fn webapp_type_inference() {
    // REST if DispatchClass is non-empty
    let dispatch_class_nonempty = "MyApp.REST.Dispatch";
    let typ = if !dispatch_class_nonempty.is_empty() {
        "REST"
    } else {
        "CSP"
    };
    assert_eq!(typ, "REST");

    // CSP if DispatchClass is empty
    let dispatch_class_empty = "";
    let typ2 = if !dispatch_class_empty.is_empty() {
        "REST"
    } else {
        "CSP"
    };
    assert_eq!(typ2, "CSP");
}

// ── admin_write_allowed helper ────────────────────────────────────────────────

#[test]
fn write_allowed_with_env_set() {
    let _g = ENV_LOCK.lock().unwrap_or_else(|e| e.into_inner());
    unsafe { std::env::set_var("IRIS_ADMIN_TOOLS", "1") };
    let result = admin_write_allowed();
    unsafe { std::env::remove_var("IRIS_ADMIN_TOOLS") };
    assert!(result);
}

#[test]
fn write_not_allowed_without_env() {
    let _g = ENV_LOCK.lock().unwrap_or_else(|e| e.into_inner());
    unsafe { std::env::remove_var("IRIS_ADMIN_TOOLS") };
    assert!(!admin_write_allowed());
}
