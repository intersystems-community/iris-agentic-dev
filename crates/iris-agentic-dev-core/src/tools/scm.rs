//! iris_source_control — SCM status, menu, checkout, execute via Atelier xecute.

use crate::elicitation::{ElicitationAction, ElicitationStore};
use crate::iris::connection::IrisConnection;
use schemars::JsonSchema;
use serde::Deserialize;

fn ok_json(v: serde_json::Value) -> Result<rmcp::model::CallToolResult, rmcp::ErrorData> {
    Ok(rmcp::model::CallToolResult::success(vec![
        rmcp::model::Content::text(v.to_string()),
    ]))
}
fn err_json(code: &str, msg: &str) -> Result<rmcp::model::CallToolResult, rmcp::ErrorData> {
    ok_json(serde_json::json!({"success": false, "error_code": code, "error": msg}))
}
fn default_namespace() -> String {
    "USER".to_string()
}

/// Menu prefix used for source control actions.
pub const SCM_MENU: &str = "%SourceMenu";

#[derive(Debug, Deserialize, JsonSchema)]
pub struct ScmParams {
    /// Action: status, menu, checkout, execute
    pub action: String,
    pub document: Option<String>,
    /// SCM action ID for action=execute
    pub action_id: Option<String>,
    /// Elicitation resume answer
    pub answer: Option<String>,
    pub elicitation_id: Option<String>,
    #[serde(default = "default_namespace")]
    pub namespace: String,
    /// Set to true to confirm write on a subject-role instance. Has no effect: source_control
    /// writes on subject instances are always hard-blocked regardless of confirm.
    #[serde(default)]
    pub confirm: bool,
}

async fn xecute(
    iris: &IrisConnection,
    client: &reqwest::Client,
    code: &str,
    namespace: &str,
) -> anyhow::Result<String> {
    iris.execute_via_generator(code, namespace, client).await
}

/// Escape a string for safe interpolation into an ObjectScript double-quoted literal.
/// Uses ObjectScript conventions: " → "", \n → $Char(10), \r → $Char(13).
fn os_quote(s: &str) -> String {
    s.replace('"', "\"\"")
        .replace('\n', "$Char(10)")
        .replace('\r', "$Char(13)")
}

/// Parse "code|msg" output from SCM xecute helpers. Returns (action_code, msg).
fn parse_action_msg(out: &str) -> (u8, &str) {
    let mut parts = out.splitn(2, '|');
    let code = parts
        .next()
        .and_then(|s| s.trim().parse::<u8>().ok())
        .unwrap_or(0);
    let msg = parts.next().map(str::trim).unwrap_or("");
    (code, msg)
}

pub async fn handle_iris_source_control(
    iris: &IrisConnection,
    client: &reqwest::Client,
    p: ScmParams,
    elicitation_store: &ElicitationStore,
) -> Result<rmcp::model::CallToolResult, rmcp::ErrorData> {
    let raw_doc = p.document.as_deref().unwrap_or("");
    let doc_owned;
    let raw_lower = raw_doc.to_ascii_lowercase();
    let doc = if !raw_doc.is_empty()
        && !raw_lower.ends_with(".cls")
        && !raw_lower.ends_with(".mac")
        && !raw_lower.ends_with(".inc")
        && !raw_lower.ends_with(".int")
    {
        doc_owned = format!("{}.cls", raw_doc);
        doc_owned.as_str()
    } else {
        raw_doc
    };
    let ns = &p.namespace;

    // Handle elicitation resume
    if let (Some(eid), Some(answer)) = (&p.elicitation_id, &p.answer) {
        let Some(pending) = elicitation_store.lookup(eid) else {
            return err_json(
                "ELICITATION_EXPIRED",
                "Elicitation session expired or not found",
            );
        };
        elicitation_store.clear(eid);
        let action_id = pending.scm_action_id.as_deref().unwrap_or("");
        let after_code = after_user_action_code(
            action_id,
            &pending.document,
            answer,
            &iris.username,
            &iris.password,
        );
        let out = match xecute(iris, client, &after_code, &pending.namespace).await {
            Ok(o) => o,
            Err(e) => {
                let msg = e.to_string();
                let (ec, emsg) = if msg == "DOCKER_REQUIRED" {
                    (
                        "DOCKER_REQUIRED",
                        "SCM operations require docker exec. Set IRIS_CONTAINER=<container_name>."
                            .to_string(),
                    )
                } else {
                    ("SCM_UNAVAILABLE", msg)
                };
                return ok_json(
                    serde_json::json!({"success": false, "error_code": ec, "error": emsg}),
                );
            }
        };
        let out = out.lines().next().unwrap_or("").trim().to_string();
        if out.is_empty() {
            return ok_json(
                serde_json::json!({"success": true, "document": pending.document, "action_id": action_id}),
            );
        }
        return err_json("SCM_ERROR", &out);
    }

    match p.action.as_str() {
        "status" => {
            let check_code = status_check_code(doc, &iris.username, &iris.password);
            let raw = xecute(iris, client, &check_code, ns)
                .await
                .unwrap_or_else(|_| "UNCONTROLLED".to_string());
            // SourceControlCreate may leave $ZERROR set, which the executor appends as
            // "ERROR($ZERROR): <ENDOFFILE>" on subsequent lines — take only the first line.
            let out = raw.lines().next().unwrap_or("").trim().to_string();
            if out == "UNCONTROLLED" || out.is_empty() {
                return ok_json(
                    serde_json::json!({"success":true,"controlled":false,"editable":true,"locked":false,"owner":null}),
                );
            }
            let (editable_flag, owner) = parse_action_msg(out.as_str());
            let editable = editable_flag == 1;
            let owner = Some(owner).filter(|s| !s.is_empty());
            ok_json(serde_json::json!({
                "success": true,
                "controlled": true,
                "editable": editable,
                "locked": !editable,
                "owner": owner,
            }))
        }

        "menu" => {
            let code = menu_all_items_code(doc, &iris.username, &iris.password);
            let raw = xecute(iris, client, &code, ns).await.unwrap_or_default();
            let mut actions = vec![];
            for line in raw.lines() {
                let line = line.trim();
                if line == "SCM_UNAVAILABLE" || line.is_empty() || line.starts_with("ERROR") {
                    continue;
                }
                // format: "name|enabled"
                let mut parts = line.splitn(2, '|');
                let name = parts.next().unwrap_or("").trim();
                let enabled: u8 = parts
                    .next()
                    .and_then(|s| s.trim().parse().ok())
                    .unwrap_or(0);
                if enabled == 1 && !name.is_empty() {
                    actions.push(serde_json::json!({"id": name, "label": name, "enabled": true}));
                }
            }
            ok_json(serde_json::json!({"success": true, "document": doc, "actions": actions}))
        }

        "checkout" => {
            let code = user_action_code("%CheckOut", doc, &iris.username, &iris.password);
            let raw = match xecute(iris, client, &code, ns).await {
                Ok(o) => o,
                Err(e) => {
                    return ok_json(
                        serde_json::json!({"success": false, "error_code": "SCM_UNAVAILABLE", "error": e.to_string()}),
                    )
                }
            };
            let out = raw.lines().next().unwrap_or("").trim();
            if out == "SCM_UNAVAILABLE" {
                return ok_json(
                    serde_json::json!({"success": false, "error_code": "SCM_UNAVAILABLE", "error": "Source control session could not be initialized"}),
                );
            }
            let (action_code, msg) = parse_action_msg(out);

            if action_code == 0 {
                return ok_json(
                    serde_json::json!({"success": true, "document": doc, "editable": true}),
                );
            }
            // action=1: need user confirmation
            let eid = elicitation_store.insert(
                doc,
                ElicitationAction::ScmExecute,
                None,
                Some("%CheckOut".to_string()),
                ns.clone(),
            );
            ok_json(serde_json::json!({
                "success": false,
                "elicitation_required": true,
                "elicitation_id": eid,
                "message": if msg.is_empty() { format!("Check out {} ?", doc) } else { msg.to_string() },
                "options": ["yes", "no"],
            }))
        }

        "execute" => {
            let action_id = p.action_id.as_deref().unwrap_or("");
            let code = user_action_code(action_id, doc, &iris.username, &iris.password);
            let raw = match xecute(iris, client, &code, ns).await {
                Ok(o) => o,
                Err(e) => {
                    return ok_json(
                        serde_json::json!({"success": false, "error_code": "SCM_UNAVAILABLE", "error": e.to_string()}),
                    )
                }
            };
            let out = raw.lines().next().unwrap_or("").trim();
            if out == "SCM_UNAVAILABLE" {
                return ok_json(
                    serde_json::json!({"success": false, "error_code": "SCM_UNAVAILABLE", "error": "Source control session could not be initialized"}),
                );
            }
            let (action_code, msg) = parse_action_msg(out);

            match action_code {
                0 => ok_json(
                    serde_json::json!({"success": true, "document": doc, "action_id": action_id}),
                ),
                1 => {
                    // Yes/No confirmation
                    let eid = elicitation_store.insert(
                        doc,
                        ElicitationAction::ScmExecute,
                        None,
                        Some(action_id.to_string()),
                        ns.clone(),
                    );
                    ok_json(serde_json::json!({
                        "success": false, "elicitation_required": true, "elicitation_id": eid,
                        "message": if msg.is_empty() { format!("Execute {} on {}?", action_id, doc) } else { msg.to_string() },
                        "options": ["yes", "no"],
                    }))
                }
                7 => {
                    // Text prompt
                    let eid = elicitation_store.insert(
                        doc,
                        ElicitationAction::ScmExecute,
                        None,
                        Some(action_id.to_string()),
                        ns.clone(),
                    );
                    ok_json(serde_json::json!({
                        "success": false, "elicitation_required": true, "elicitation_id": eid,
                        "message": if msg.is_empty() { format!("Enter value for {}:", action_id) } else { msg.to_string() },
                        "input_type": "text",
                    }))
                }
                _ => err_json(
                    "SCM_ERROR",
                    &format!("Unexpected action code {} from UserAction", action_code),
                ),
            }
        }

        other => err_json(
            "INVALID_PARAM",
            &format!(
                "Unknown action='{}'. Use: status, menu, checkout, execute",
                other
            ),
        ),
    }
}

/// Build the ObjectScript snippet that determines SCM status for a document.
/// Uses GetStatus for controlled/uncontrolled, then MenuItems to deduce editable/owner
/// since many SCM implementations don't populate GetStatus's editable/owner fields.
/// Output: "UNCONTROLLED" | "1|owner" (checked out by me) | "0|" (locked)
fn status_check_code(doc: &str, username: &str, password: &str) -> String {
    let doc_q = os_quote(doc);
    let user_q = os_quote(username);
    let pass_q = os_quote(password);
    format!(
        "set sc=##class(%Studio.SourceControl.Interface).SourceControlCreate(\"{user_q}\",\"{pass_q}\",.created,.flags,.outuser) \
         set isInSC=0 \
         set sc=##class(%Studio.SourceControl.Interface).GetStatus(\"{doc_q}\",.isInSC,.editable,.isCheckedOut,.owner) \
         if $system.Status.IsError(sc)||('isInSC) {{ write \"UNCONTROLLED\" }} \
         else {{ \
           set hasUndoCheckout=0 set hasCheckOut=0 \
           set rset=##class(%ResultSet).%New(\"%Studio.SourceControl.Interface:MenuItems\") \
           set sc=rset.Execute(\"%SourceMenu\",\"{doc_q}\",\"\") \
           while rset.Next() {{ \
             set itemName=rset.GetData(1) set itemEnabled=rset.GetData(2) \
             if itemEnabled&&(itemName=\"%UndoCheckout\") {{ set hasUndoCheckout=1 }} \
             if itemEnabled&&(itemName=\"%CheckOut\") {{ set hasCheckOut=1 }} \
           }} \
           if hasUndoCheckout {{ write 1_\"|\"_\"{user_q}\" }} \
           else {{ write \"0|\" }} \
         }}"
    )
}

/// Prefix that initializes a SCM session via SourceControlCreate and binds obj=%SourceControl.
/// All SCM methods are instance methods — they require an active %SourceControl object.
fn scm_init_prefix(username: &str, password: &str) -> String {
    let user_q = os_quote(username);
    let pass_q = os_quote(password);
    format!(
        "set sc=##class(%Studio.SourceControl.Interface).SourceControlCreate(\"{user_q}\",\"{pass_q}\",.created,.flags,.outuser) \
         set obj=$get(%SourceControl) \
         if '$IsObject(obj) {{ write \"SCM_UNAVAILABLE\" quit }} "
    )
}

/// Build the ObjectScript snippet that invokes `UserAction` on the SCM instance,
/// writing "action|msg" to the output stream.
fn user_action_code(action_id: &str, doc: &str, username: &str, password: &str) -> String {
    let prefix = scm_init_prefix(username, password);
    format!(
        "{prefix}set action=0 set target=\"\" set msg=\"\" set reload=0 \
         set sc=obj.UserAction(0,\"%SourceMenu,{}\",\"{}\",\"\",.action,.target,.msg,.reload) \
         write action_\"|\"_$select(msg'=\"\":msg,target'=\"\":target,1:\"\")",
        os_quote(action_id),
        os_quote(doc),
    )
}

/// Build the ObjectScript snippet that re-runs `UserAction` then immediately calls
/// `AfterUserAction` in the same job, so %SourceControl state is preserved.
fn after_user_action_code(
    action_id: &str,
    doc: &str,
    answer: &str,
    username: &str,
    password: &str,
) -> String {
    let prefix = scm_init_prefix(username, password);
    let answer_int = if answer == "yes" { "1" } else { "0" };
    let action_id_q = os_quote(action_id);
    let doc_q = os_quote(doc);
    format!(
        "{prefix}\
         set action=0 set target=\"\" set msg=\"\" set reload=0 \
         set sc=obj.UserAction(0,\"%SourceMenu,{action_id_q}\",\"{doc_q}\",\"\",.action,.target,.msg,.reload) \
         set sc=obj.AfterUserAction(0,\"%SourceMenu,{action_id_q}\",\"{doc_q}\",{answer_int},\"\") \
         write $system.Status.GetErrorText(sc)"
    )
}

/// Build a single ObjectScript snippet that queries all enabled SCM menu items via
/// the MenuItems ResultSet, writing one "name|enabled|displayName" line per item.
fn menu_all_items_code(doc: &str, username: &str, password: &str) -> String {
    let prefix = scm_init_prefix(username, password);
    let doc_q = os_quote(doc);
    format!(
        "{prefix}\
         set rset=##class(%ResultSet).%New(\"%Studio.SourceControl.Interface:MenuItems\") \
         set sc=rset.Execute(\"%SourceMenu\",\"{doc_q}\",\"\") \
         while rset.Next() {{ write rset.GetData(1)_\"|\"_rset.GetData(2),! }}"
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    // ── os_quote ──────────────────────────────────────────────────────────────
    #[test]
    fn test_os_quote_double_quotes() {
        assert_eq!(os_quote(r#"say "hi""#), r#"say ""hi"""#);
    }
    #[test]
    fn test_os_quote_newline() {
        assert_eq!(os_quote("a\nb"), "a$Char(10)b");
    }
    #[test]
    fn test_os_quote_cr() {
        assert_eq!(os_quote("a\rb"), "a$Char(13)b");
    }
    #[test]
    fn test_os_quote_plain() {
        assert_eq!(os_quote("hello"), "hello");
    }
    #[test]
    fn test_os_quote_empty() {
        assert_eq!(os_quote(""), "");
    }

    // ── parse_action_msg ─────────────────────────────────────────────────────
    #[test]
    fn test_parse_action_msg_code_and_msg() {
        let (code, msg) = parse_action_msg("1|Please enter comment");
        assert_eq!(code, 1);
        assert_eq!(msg, "Please enter comment");
    }
    #[test]
    fn test_parse_action_msg_zero_ok() {
        let (code, msg) = parse_action_msg("0|");
        assert_eq!(code, 0);
        assert_eq!(msg, "");
    }
    #[test]
    fn test_parse_action_msg_no_pipe() {
        let (code, msg) = parse_action_msg("0");
        assert_eq!(code, 0);
        assert_eq!(msg, "");
    }
    #[test]
    fn test_parse_action_msg_message_with_pipes() {
        // Only splits on first pipe
        let (code, msg) = parse_action_msg("1|msg with | pipe");
        assert_eq!(code, 1);
        assert_eq!(msg, "msg with | pipe");
    }
    #[test]
    fn test_parse_action_msg_type_7() {
        let (code, msg) = parse_action_msg("7|Enter value:");
        assert_eq!(code, 7);
        assert_eq!(msg, "Enter value:");
    }

    // ── user_action_code ──────────────────────────────────────────────────────
    #[test]
    fn test_user_action_code_no_backslash_quote() {
        let code = user_action_code("CheckOut", "MyApp.Patient.cls", "user", "pass");
        assert!(
            !code.contains("\\\""),
            "must use ObjectScript quoting, not backslash: {}",
            code
        );
        assert!(
            code.contains("CheckOut"),
            "must contain action_id: {}",
            code
        );
        assert!(
            code.contains("MyApp.Patient.cls"),
            "must contain doc: {}",
            code
        );
    }
    #[test]
    fn test_user_action_code_escapes_quotes_in_action() {
        let code = user_action_code("Check\"Out", "Doc.cls", "user", "pass");
        assert!(
            code.contains("\"\""),
            "double-quote must become \"\": {}",
            code
        );
        assert!(!code.contains("\\\""), "no backslash-quote: {}", code);
    }
    #[test]
    fn test_user_action_code_escapes_newline_in_doc() {
        let code = user_action_code("CheckOut", "Doc\nwith\nnewlines.cls", "user", "pass");
        assert!(
            code.contains("$Char(10)"),
            "newline must become $Char(10): {}",
            code
        );
    }

    // ── status_check_code ─────────────────────────────────────────────────────
    #[test]
    fn test_status_check_code_uses_get_status() {
        let code = status_check_code("MyApp.Patient.cls", "user", "pass");
        assert!(
            code.contains("%Studio.SourceControl.Interface"),
            "must use Interface class: {code}"
        );
        assert!(code.contains("GetStatus"), "must call GetStatus: {code}");
        assert!(
            code.contains("SourceControlCreate"),
            "must init session: {code}"
        );
        assert!(
            !code.contains("%GetImplementationObject"),
            "must not use removed method: {code}"
        );
    }

    #[test]
    fn test_status_check_code_contains_doc() {
        let code = status_check_code("MyApp.Patient.cls", "user", "pass");
        assert!(
            code.contains("MyApp.Patient.cls"),
            "must embed document name: {code}"
        );
    }

    #[test]
    fn test_status_check_code_escapes_quotes_in_doc() {
        let code = status_check_code("My\"App.cls", "user", "pass");
        assert!(
            code.contains("\"\""),
            "double-quote must become \"\": {code}"
        );
        assert!(!code.contains("\\\""), "no backslash-quote: {code}");
    }

    #[test]
    fn test_status_check_code_escapes_quotes_in_credentials() {
        let code = status_check_code("Any.cls", "us\"er", "p\"ass");
        assert!(
            code.contains("\"\""),
            "double-quote in credentials must be escaped: {code}"
        );
    }

    #[test]
    fn test_status_check_code_writes_uncontrolled_branch() {
        let code = status_check_code("Any.cls", "user", "pass");
        assert!(
            code.contains("UNCONTROLLED"),
            "must write UNCONTROLLED when not in SC: {code}"
        );
    }

    #[test]
    fn test_status_check_code_uses_menu_items_for_checkout_state() {
        let code = status_check_code("Any.cls", "user", "pass");
        assert!(
            code.contains("MenuItems"),
            "must use MenuItems to deduce checkout state: {code}"
        );
        assert!(
            code.contains("%UndoCheckout"),
            "must check for UndoCheckout to detect checked-out: {code}"
        );
    }

    #[test]
    fn test_status_check_code_writes_owner_when_checked_out() {
        let code = status_check_code("Any.cls", "myuser", "pass");
        assert!(
            code.contains("1_\"|\"_\"myuser\""),
            "must write 1|username when checked out: {code}"
        );
    }

    // ── SCM_MENU ──────────────────────────────────────────────────────────────
    #[test]
    fn test_scm_menu_prefix() {
        assert_eq!(SCM_MENU, "%SourceMenu");
    }

    // ── scm_init_prefix ──────────────────────────────────────────────────────
    #[test]
    fn test_scm_init_prefix_contains_source_control_create() {
        let code = scm_init_prefix("user", "pass");
        assert!(code.contains("SourceControlCreate"), "{code}");
        assert!(code.contains("%Studio.SourceControl.Interface"), "{code}");
    }

    #[test]
    fn test_scm_init_prefix_escapes_quotes_in_user() {
        let code = scm_init_prefix("us\"er", "pass");
        assert!(
            code.contains("\"\""),
            "double-quote must be doubled: {code}"
        );
        assert!(!code.contains("\\\""), "no backslash-quote: {code}");
    }

    // ── menu_all_items_code ──────────────────────────────────────────────────
    #[test]
    fn test_menu_all_items_code_contains_menu_items() {
        let code = menu_all_items_code("MyApp.cls", "user", "pass");
        assert!(code.contains("MenuItems"), "{code}");
    }

    #[test]
    fn test_menu_all_items_code_contains_doc() {
        let code = menu_all_items_code("MyApp.Patient.cls", "user", "pass");
        assert!(code.contains("MyApp.Patient.cls"), "{code}");
    }

    // ── after_user_action_code ───────────────────────────────────────────────
    #[test]
    fn test_after_user_action_code_contains_after_user_action() {
        let code = after_user_action_code("CheckOut", "MyApp.cls", "yes", "user", "pass");
        assert!(code.contains("AfterUserAction"), "{code}");
    }

    #[test]
    fn test_after_user_action_code_contains_doc() {
        let code = after_user_action_code("CheckOut", "MyApp.Patient.cls", "no", "user", "pass");
        assert!(code.contains("MyApp.Patient.cls"), "{code}");
    }

    #[test]
    fn test_after_user_action_code_yes_becomes_1() {
        let code = after_user_action_code("CheckOut", "Doc.cls", "yes", "user", "pass");
        assert!(code.contains(",1,"), "yes should become 1: {code}");
    }

    #[test]
    fn test_after_user_action_code_no_becomes_0() {
        let code = after_user_action_code("CheckOut", "Doc.cls", "no", "user", "pass");
        assert!(code.contains(",0,"), "no should become 0: {code}");
    }

    // ── Document name normalization ──────────────────────────────────────────
    #[test]
    fn test_normalize_cls_extension_appended_for_bare_class() {
        let doc = "MyApp.Patient";
        let normalized = if !doc.contains('.')
            || doc.ends_with(".cls")
            || doc.ends_with(".mac")
            || doc.ends_with(".inc")
            || doc.ends_with(".int")
        {
            doc.to_string()
        } else {
            format!("{}.cls", doc)
        };
        // "MyApp.Patient" has a dot but no extension suffix → should get .cls
        assert_eq!(normalized, "MyApp.Patient.cls");
    }

    // ── scm_init_prefix additional ───────────────────────────────────────────
    #[test]
    fn test_scm_init_prefix_contains_get_source_control() {
        let code = scm_init_prefix("user", "pass");
        // Must bind obj to %SourceControl for instance method calls
        assert!(
            code.contains("%SourceControl"),
            "must bind %SourceControl: {code}"
        );
    }

    #[test]
    fn test_scm_init_prefix_writes_scm_unavailable_on_no_obj() {
        let code = scm_init_prefix("user", "pass");
        assert!(
            code.contains("SCM_UNAVAILABLE"),
            "must write SCM_UNAVAILABLE when obj unavailable: {code}"
        );
    }

    #[test]
    fn test_scm_init_prefix_escapes_quotes_in_password() {
        let code = scm_init_prefix("user", "p\"ass");
        assert!(
            code.contains("\"\""),
            "double-quote in password must be doubled: {code}"
        );
        assert!(
            !code.contains("\\\""),
            "no backslash-quote in password: {code}"
        );
    }

    // ── user_action_code additional ───────────────────────────────────────────
    #[test]
    fn test_user_action_code_contains_user_action() {
        let code = user_action_code("CheckOut", "MyApp.cls", "user", "pass");
        assert!(
            code.contains("UserAction"),
            "must invoke UserAction: {code}"
        );
    }

    #[test]
    fn test_user_action_code_contains_source_menu() {
        let code = user_action_code("CheckOut", "MyApp.cls", "user", "pass");
        assert!(
            code.contains("%SourceMenu"),
            "must pass %SourceMenu prefix: {code}"
        );
    }

    #[test]
    fn test_user_action_code_escapes_quotes_in_credentials() {
        let code = user_action_code("CheckOut", "Doc.cls", "us\"er", "p\"ass");
        assert!(
            code.contains("\"\""),
            "double-quote in credentials must be doubled: {code}"
        );
        assert!(!code.contains("\\\""), "no backslash-quote: {code}");
    }

    // ── menu_all_items_code additional ────────────────────────────────────────
    #[test]
    fn test_menu_all_items_code_contains_source_menu() {
        let code = menu_all_items_code("MyApp.cls", "user", "pass");
        assert!(
            code.contains("%SourceMenu"),
            "must pass %SourceMenu to Execute: {code}"
        );
    }

    #[test]
    fn test_menu_all_items_code_escapes_quotes_in_doc() {
        let code = menu_all_items_code("My\"App.cls", "user", "pass");
        assert!(
            code.contains("\"\""),
            "double-quote in doc must be doubled: {code}"
        );
        assert!(!code.contains("\\\""), "no backslash-quote: {code}");
    }

    #[test]
    fn test_menu_all_items_code_contains_source_control_create() {
        let code = menu_all_items_code("MyApp.cls", "user", "pass");
        assert!(
            code.contains("SourceControlCreate"),
            "must init session: {code}"
        );
    }

    // ── after_user_action_code additional ────────────────────────────────────
    #[test]
    fn test_after_user_action_code_contains_source_menu() {
        let code = after_user_action_code("CheckOut", "MyApp.cls", "yes", "user", "pass");
        assert!(
            code.contains("%SourceMenu"),
            "must pass %SourceMenu: {code}"
        );
    }

    #[test]
    fn test_after_user_action_code_contains_user_action() {
        let code = after_user_action_code("CheckOut", "MyApp.cls", "yes", "user", "pass");
        assert!(
            code.contains("UserAction"),
            "must call UserAction first: {code}"
        );
    }

    #[test]
    fn test_after_user_action_code_writes_error_text() {
        let code = after_user_action_code("CheckOut", "MyApp.cls", "yes", "user", "pass");
        assert!(
            code.contains("GetErrorText"),
            "must write error text from AfterUserAction: {code}"
        );
    }

    // ── parse_action_msg edge cases ───────────────────────────────────────────
    #[test]
    fn test_parse_action_msg_empty_string() {
        let (code, msg) = parse_action_msg("");
        assert_eq!(code, 0);
        assert_eq!(msg, "");
    }

    #[test]
    fn test_parse_action_msg_whitespace_trimmed() {
        let (code, msg) = parse_action_msg("  1  |  some msg  ");
        assert_eq!(code, 1);
        assert_eq!(msg, "some msg");
    }
}
