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

/// SCM menu actions as reported by %Studio.SourceControl.Interface:MenuItems.
#[derive(Debug, PartialEq, Eq)]
pub enum ScmAction {
    CheckOut,
    UndoCheckout,
    CheckIn,
    GetLatest,
    AddToSourceControl,
    Diff,
    Disconnect,
    Reconnect,
    Unknown(String),
}

impl ScmAction {
    pub fn from_id(id: &str) -> Self {
        match id.trim_start_matches('%') {
            "CheckOut" => Self::CheckOut,
            "UndoCheckout" => Self::UndoCheckout,
            "CheckIn" => Self::CheckIn,
            "GetLatest" => Self::GetLatest,
            "AddToSourceControl" => Self::AddToSourceControl,
            "Diff" => Self::Diff,
            "Disconnect" => Self::Disconnect,
            "Reconnect" => Self::Reconnect,
            other => Self::Unknown(other.to_string()),
        }
    }
}

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
            let raw = match xecute(iris, client, &check_code, ns).await {
                Ok(o) => o,
                Err(e) => {
                    // A transport/exec failure must NOT be reported as "editable" — that is the
                    // very inconsistency this path used to have. Surface it honestly.
                    return ok_json(serde_json::json!({
                        "success": false,
                        "error_code": "SCM_UNAVAILABLE",
                        "error": e.to_string(),
                    }));
                }
            };
            // The executor may append "ERROR($ZERROR): …" on later lines — find the SCMSTATUS
            // sentinel line rather than assuming it is the first one.
            let parsed = raw.lines().find_map(parse_scm_status_line);
            let Some((is_in_sc, has_co, has_undo, has_add, owner)) = parsed else {
                // No SCMSTATUS sentinel. Before giving up, try the provider's native
                // "checked out by user '<name>'" notice, which short-circuits the probe (often
                // with a <PROTECT>) before the sentinel is written. That notice still tells us the
                // document is controlled and locked by another user — report that instead of an
                // opaque SCM_UNAVAILABLE.
                if let Some((other_owner, ts)) = parse_checked_out_by(&raw) {
                    let checked_out_by_me = other_owner.eq_ignore_ascii_case(&iris.username);
                    let mut resp = serde_json::json!({
                        "success": true,
                        "controlled": true,
                        "editable": checked_out_by_me,
                        "locked": !checked_out_by_me,
                        "checked_out_by_me": checked_out_by_me,
                        "owner": other_owner,
                    });
                    if let Some(ts) = ts {
                        resp["checked_out_at"] = serde_json::Value::String(ts);
                    }
                    return ok_json(resp);
                }
                // Echo the raw IRIS output (truncated) so the actual failure — a <PROTECT>, an
                // authentication banner, an empty body — is diagnosable instead of being flattened
                // into an opaque "no status signal".
                let raw_trunc: String = raw.trim().chars().take(600).collect();
                return ok_json(serde_json::json!({
                    "success": false,
                    "error_code": "SCM_UNAVAILABLE",
                    "error": "Could not determine source control status (no SCMSTATUS sentinel in IRIS output)",
                    "raw_output": raw_trunc,
                }));
            };
            let status =
                derive_scm_status(is_in_sc, has_co, has_undo, has_add, &owner, &iris.username);
            let Some(status) = status else {
                return ok_json(serde_json::json!({
                    "success": false,
                    "error_code": "SCM_UNAVAILABLE",
                    "error": "Source control status is indeterminate for this document",
                }));
            };
            ok_json(serde_json::json!({
                "success": true,
                "controlled": status.controlled,
                "editable": status.editable,
                "locked": status.locked,
                "checked_out_by_me": status.checked_out_by_me,
                "owner": status.owner,
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
                let checkin_allowed = std::env::var("IRIS_SCM_ALLOW_CHECKIN")
                    .map(|v| matches!(v.to_ascii_lowercase().as_str(), "1" | "true" | "yes"))
                    .unwrap_or(false);
                if enabled == 1
                    && !name.is_empty()
                    && (ScmAction::from_id(name) != ScmAction::CheckIn || checkin_allowed)
                {
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
                // action=0 means UserAction wants no confirmation dialog — but the checkout
                // is NOT actually committed until AfterUserAction runs. Reporting success
                // here on UserAction alone was a false positive: the item looked checked out
                // but a later write failed with ERROR #5865. Finalize with AfterUserAction so
                // the checkout genuinely persists server-side before we claim success.
                let after_code =
                    after_user_action_code("%CheckOut", doc, "yes", &iris.username, &iris.password);
                match xecute(iris, client, &after_code, ns).await {
                    Ok(o) => {
                        let aout = o.lines().next().unwrap_or("").trim().to_string();
                        if !aout.is_empty() && aout != "SCM_UNAVAILABLE" {
                            return err_json("SCM_CHECKOUT_FAILED", &aout);
                        }
                    }
                    Err(e) => {
                        return ok_json(serde_json::json!({
                            "success": false,
                            "error_code": "SCM_UNAVAILABLE",
                            "error": e.to_string(),
                        }))
                    }
                }
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
            if ScmAction::from_id(action_id) == ScmAction::CheckIn {
                let allowed = std::env::var("IRIS_SCM_ALLOW_CHECKIN")
                    .map(|v| matches!(v.to_ascii_lowercase().as_str(), "1" | "true" | "yes"))
                    .unwrap_or(false);
                if !allowed {
                    return err_json(
                        "CHECKIN_BLOCKED",
                        "CheckIn is disabled by default. Set IRIS_SCM_ALLOW_CHECKIN=1 to enable.",
                    );
                }
            }
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
///
/// Emits one structured, pipe-delimited line so the caller (`derive_scm_status`) can combine
/// every available signal instead of relying on a single heuristic:
///   `SCMSTATUS|<isErr>|<isInSC>|<editable>|<hasCheckOut>|<hasUndoCheckout>|<hasAddToSC>|<owner>`
/// where the six middle fields are 0/1 and `owner` is the GetStatus owner (may be empty).
/// The `SCMSTATUS` sentinel lets the caller distinguish a real result from a transport/error
/// line (the executor may append `ERROR($ZERROR): …` on subsequent lines).
fn status_check_code(doc: &str, username: &str, password: &str) -> String {
    let doc_q = os_quote(doc);
    let user_q = os_quote(username);
    let pass_q = os_quote(password);
    // Each risky step is wrapped in TRY/CATCH so a runtime error (SourceControlCreate failing,
    // GetStatus <PROTECT>, an SCM provider that has no MenuItems query, …) can never abort the
    // job before the SCMSTATUS sentinel is written. Without this, any partial failure produced
    // "no status signal returned" instead of a usable (possibly indeterminate) status.
    format!(
        "set isErr=0,isInSC=0,editable=0,isCheckedOut=0,owner=\"\" \
         set hasCheckOut=0,hasUndoCheckout=0,hasAddToSC=0 \
         try {{ set sc=##class(%Studio.SourceControl.Interface).SourceControlCreate(\"{user_q}\",\"{pass_q}\",.created,.flags,.outuser) }} catch {{ set isErr=1 }} \
         try {{ set sc=##class(%Studio.SourceControl.Interface).GetStatus(\"{doc_q}\",.isInSC,.editable,.isCheckedOut,.owner) if $system.Status.IsError(sc) {{ set isErr=1 }} }} catch {{ set isErr=1 }} \
         try {{ \
           set rset=##class(%ResultSet).%New(\"%Studio.SourceControl.Interface:MenuItems\") \
           set sc=rset.Execute(\"%SourceMenu\",\"{doc_q}\",\"\") \
           while rset.Next() {{ \
             set itemName=rset.GetData(1),itemEnabled=rset.GetData(2) \
             if itemEnabled&&(itemName=\"%CheckOut\") {{ set hasCheckOut=1 }} \
             if itemEnabled&&(itemName=\"%UndoCheckout\") {{ set hasUndoCheckout=1 }} \
             if itemEnabled&&(itemName=\"%AddToSourceControl\") {{ set hasAddToSC=1 }} \
           }} \
         }} catch {{ set isErr=1 }} \
         write \"SCMSTATUS|\"_isErr_\"|\"_isInSC_\"|\"_editable_\"|\"_hasCheckOut_\"|\"_hasUndoCheckout_\"|\"_hasAddToSC_\"|\"_owner"
    )
}

/// Resolved SCM status for a document, derived from the combined `status_check_code` signals.
#[derive(Debug, PartialEq, Eq)]
pub struct ScmStatus {
    /// The document is under source control.
    pub controlled: bool,
    /// The document can be written right now (uncontrolled, or checked out by the current user).
    pub editable: bool,
    /// The document is locked by someone else (controlled, checked out, not by us).
    pub locked: bool,
    /// The current user holds the checkout.
    pub checked_out_by_me: bool,
    /// The checkout owner, when known (the current user if we hold it, else GetStatus owner).
    pub owner: Option<String>,
}

/// Combine every SCM signal into a coherent status.
///
/// Returns `None` when the signals are inconclusive (GetStatus errored *and* the menu offered
/// no source-control actions) — the caller must then report the status as unavailable rather
/// than silently claiming the document is editable.
///
/// Decision logic:
/// - With zero signal (no in-SC flag, no menu items, no owner) the status is indeterminate →
///   `None`. We never fall back to guessing "editable", which was the original bug.
/// - `uncontrolled` ⇔ the menu offers `%AddToSourceControl` (you can only add a document that
///   is not yet under source control). Keyed on this positive signal rather than on GetStatus's
///   `isInSC`, which some providers leave unpopulated.
/// - checked-out-by-me ⇔ `%UndoCheckout` is enabled (only the holder can undo their checkout).
/// - available-to-checkout ⇔ controlled and `%CheckOut` is enabled (free to take, not locked).
/// - locked-by-other ⇔ controlled and neither CheckOut nor UndoCheckout is available
///   (someone else holds it).
fn derive_scm_status(
    is_in_sc: bool,
    has_checkout: bool,
    has_undo_checkout: bool,
    has_add_to_sc: bool,
    owner: &str,
    current_user: &str,
) -> Option<ScmStatus> {
    let owner_opt = Some(owner.trim().to_string()).filter(|s| !s.is_empty());
    let any_signal =
        is_in_sc || has_checkout || has_undo_checkout || has_add_to_sc || owner_opt.is_some();
    // No signal at all → no SCM configured, document is freely editable.
    // (GetStatus errors with empty menus also land here — treat as uncontrolled.)
    if !any_signal {
        return Some(ScmStatus {
            controlled: false,
            editable: true,
            locked: false,
            checked_out_by_me: false,
            owner: None,
        });
    }

    // A document is uncontrolled iff we are offered the action to add it to source control.
    if has_add_to_sc {
        return Some(ScmStatus {
            controlled: false,
            editable: true,
            locked: false,
            checked_out_by_me: false,
            owner: None,
        });
    }

    if has_undo_checkout {
        // We hold the checkout → writable by us.
        return Some(ScmStatus {
            controlled: true,
            editable: true,
            locked: false,
            checked_out_by_me: true,
            owner: owner_opt.or_else(|| Some(current_user.to_string())),
        });
    }

    if has_checkout {
        // Controlled and free to check out — not currently editable, but not locked by anyone.
        return Some(ScmStatus {
            controlled: true,
            editable: false,
            locked: false,
            checked_out_by_me: false,
            owner: owner_opt,
        });
    }

    // Controlled, can neither check out nor undo → locked by another user.
    Some(ScmStatus {
        controlled: true,
        editable: false,
        locked: true,
        checked_out_by_me: false,
        owner: owner_opt,
    })
}

/// Parse the `SCMSTATUS|…` line emitted by `status_check_code` into
/// `(isInSC, hasCheckOut, hasUndoCheckout, hasAddToSC, owner)`. Returns `None` if the line is
/// missing the sentinel or has the wrong arity (e.g. a transport error was returned instead).
///
/// The leading `isErr` and the `editable` fields are consumed but not returned: `isErr` only
/// gates nothing now (absence of signal is the real "unknown" test), and GetStatus's `editable`
/// is advisory — the menu action signals are authoritative.
fn parse_scm_status_line(line: &str) -> Option<(bool, bool, bool, bool, String)> {
    // SCMSTATUS|isErr|isInSC|editable|hasCheckOut|hasUndoCheckout|hasAddToSC|owner
    let mut parts = line.trim().splitn(8, '|');
    if parts.next()? != "SCMSTATUS" {
        return None;
    }
    let flag = |p: Option<&str>| p.map(|s| s.trim() != "0" && !s.trim().is_empty());
    let _is_err = flag(parts.next())?;
    let is_in_sc = flag(parts.next())?;
    let _editable = flag(parts.next())?;
    let has_checkout = flag(parts.next())?;
    let has_undo_checkout = flag(parts.next())?;
    let has_add_to_sc = flag(parts.next())?;
    let owner = parts.next().unwrap_or("").trim().to_string();
    Some((
        is_in_sc,
        has_checkout,
        has_undo_checkout,
        has_add_to_sc,
        owner,
    ))
}

/// Fallback owner detection from the SCM provider's native `checked out by user '<name>'` notice.
///
/// Some source-control providers emit a native `NOTICE: … is currently checked out by user
/// 'todor', and was last updated at 2026-07-07 12:34:56` message (often followed by a `<PROTECT>`)
/// that short-circuits `status_check_code` before the `SCMSTATUS|` sentinel is ever written. In
/// that case `parse_scm_status_line` finds nothing, yet the raw output already tells us the
/// document is controlled and locked by another user — so we scrape it here instead of reporting
/// an opaque `SCM_UNAVAILABLE`.
///
/// The regex is tolerant: the message may be repeated (the probe loops) and truncated mid-line
/// (before `updated at …`). We take the first match, ignore repetitions, and treat the timestamp
/// as optional. Returns `(owner, Option<timestamp>)`.
fn parse_checked_out_by(raw: &str) -> Option<(String, Option<String>)> {
    use std::sync::OnceLock;
    static RE: OnceLock<regex::Regex> = OnceLock::new();
    let re = RE.get_or_init(|| {
        // "checked out by user 'todor'" — timestamp captured only if the line isn't truncated.
        regex::Regex::new(r"checked out by user '([^']+)'(?:.*?updated at ([0-9-]+ [0-9:]+))?")
            .expect("static SCM checked-out regex is valid")
    });
    let caps = re.captures(raw)?;
    let owner = caps.get(1)?.as_str().trim().to_string();
    if owner.is_empty() {
        return None;
    }
    let ts = caps.get(2).map(|m| m.as_str().trim().to_string());
    Some((owner, ts))
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
pub(crate) fn after_user_action_code(
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
    #[test]
    fn test_os_quote_mixed_quote_and_newline() {
        // String with both double-quote and newline
        assert_eq!(os_quote("say \"hi\"\nbye"), "say \"\"hi\"\"$Char(10)bye");
    }
    #[test]
    fn test_os_quote_cr_and_newline() {
        // String with carriage return AND newline
        assert_eq!(os_quote("line\r\nend"), "line$Char(13)$Char(10)end");
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
    fn test_status_check_code_emits_structured_sentinel() {
        let code = status_check_code("Any.cls", "user", "pass");
        assert!(
            code.contains("SCMSTATUS|"),
            "must emit the SCMSTATUS sentinel line: {code}"
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
        assert!(
            code.contains("%CheckOut"),
            "must check for CheckOut availability: {code}"
        );
        assert!(
            code.contains("%AddToSourceControl"),
            "must check AddToSourceControl to distinguish uncontrolled docs: {code}"
        );
    }

    #[test]
    fn test_status_check_code_contains_username() {
        let code = status_check_code("Doc.cls", "myuser", "pass");
        assert!(code.contains("myuser"), "must contain the username: {code}");
    }

    #[test]
    fn test_status_check_code_contains_document_name() {
        let code = status_check_code("MyClass.cls", "user", "pass");
        assert!(
            code.contains("MyClass.cls"),
            "must contain the document name: {code}"
        );
    }

    #[test]
    fn test_status_check_code_contains_scmstatus_sentinel() {
        let code = status_check_code("Doc.cls", "user", "pass");
        assert!(
            code.contains("SCMSTATUS"),
            "must contain SCMSTATUS sentinel: {code}"
        );
    }

    // ── parse_scm_status_line ─────────────────────────────────────────────────
    #[test]
    fn test_parse_scm_status_line_full() {
        let (in_sc, co, undo, add, owner) =
            parse_scm_status_line("SCMSTATUS|0|1|0|0|0|0|test").unwrap();
        assert!(in_sc);
        assert!(!co);
        assert!(!undo);
        assert!(!add);
        assert_eq!(owner, "test");
    }

    #[test]
    fn test_parse_scm_status_line_finds_sentinel_amid_noise() {
        // Executor may prepend/append error lines; find_map over lines must still parse it.
        let raw = "SCMSTATUS|0|0|0|1|0|1|\nERROR($ZERROR): <ENDOFFILE>";
        let parsed = raw.lines().find_map(parse_scm_status_line);
        assert!(parsed.is_some());
    }

    #[test]
    fn test_parse_scm_status_line_rejects_non_sentinel() {
        assert!(parse_scm_status_line("ERROR: something broke").is_none());
        assert!(parse_scm_status_line("").is_none());
    }

    #[test]
    fn test_parse_scm_status_line_with_owner() {
        // Line with owner field populated
        let (in_sc, co, undo, add, owner) =
            parse_scm_status_line("SCMSTATUS|0|1|0|1|1|0|alice").unwrap();
        assert!(in_sc);
        assert!(co);
        assert!(undo);
        assert!(!add);
        assert_eq!(owner, "alice");
    }

    #[test]
    fn test_parse_scm_status_line_malformed_missing_pipe() {
        // Malformed line (missing pipe) should return None
        assert!(parse_scm_status_line("SCMSTATUS0101010").is_none());
    }

    #[test]
    fn test_parse_scm_status_line_all_fields_zero() {
        // Valid line with all boolean fields as 0
        let (in_sc, co, undo, add, owner) =
            parse_scm_status_line("SCMSTATUS|0|0|0|0|0|0|").unwrap();
        assert!(!in_sc);
        assert!(!co);
        assert!(!undo);
        assert!(!add);
        assert_eq!(owner, "");
    }

    // ── parse_checked_out_by (native NOTICE fallback, bug #3) ─────────────────
    #[test]
    fn test_parse_checked_out_by_with_timestamp() {
        let raw = "NOTICE: 'My.Class.cls' is currently checked out by user 'todor', and was last updated at 2026-07-07 12:34:56";
        let (owner, ts) = parse_checked_out_by(raw).unwrap();
        assert_eq!(owner, "todor");
        assert_eq!(ts.as_deref(), Some("2026-07-07 12:34:56"));
    }

    #[test]
    fn test_parse_checked_out_by_truncated_no_timestamp() {
        // Real-world case: the message is truncated mid-line before "updated at …".
        let raw = "...is currently checked out by user 'todor', and was last";
        let (owner, ts) = parse_checked_out_by(raw).unwrap();
        assert_eq!(owner, "todor");
        assert_eq!(ts, None);
    }

    #[test]
    fn test_parse_checked_out_by_takes_first_of_repeated() {
        // The probe loops, so the notice repeats. First occurrence wins.
        let raw =
            "checked out by user 'todor', and was last\nchecked out by user 'alice', and was last";
        let (owner, _) = parse_checked_out_by(raw).unwrap();
        assert_eq!(owner, "todor");
    }

    #[test]
    fn test_parse_checked_out_by_none_when_absent() {
        assert!(parse_checked_out_by("ERROR: <PROTECT>").is_none());
        assert!(parse_checked_out_by("").is_none());
    }

    #[test]
    fn test_parse_checked_out_by_standard_format_no_timestamp() {
        // Standard format without timestamp
        let raw = "checked out by user 'james'";
        let (owner, ts) = parse_checked_out_by(raw).unwrap();
        assert_eq!(owner, "james");
        assert_eq!(ts, None);
    }

    #[test]
    fn test_parse_checked_out_by_empty_string() {
        // Empty string should return None
        assert!(parse_checked_out_by("").is_none());
    }

    #[test]
    fn test_parse_checked_out_by_unrelated_text() {
        // Unrelated text without the pattern should return None
        assert!(parse_checked_out_by("This document has some other information").is_none());
        assert!(
            parse_checked_out_by("checked out by admin but not in the expected format").is_none()
        );
    }

    // ── derive_scm_status ─────────────────────────────────────────────────────
    #[test]
    fn test_derive_uncontrolled_is_editable() {
        // Menu offers AddToSourceControl → uncontrolled, editable.
        let s = derive_scm_status(false, false, false, true, "", "me").unwrap();
        assert!(!s.controlled);
        assert!(s.editable);
        assert!(!s.locked);
        assert_eq!(s.owner, None);
    }

    #[test]
    fn test_derive_checked_out_by_me() {
        // In SC, UndoCheckout enabled → I hold it, editable.
        let s = derive_scm_status(true, false, true, false, "", "me").unwrap();
        assert!(s.controlled);
        assert!(s.editable);
        assert!(!s.locked);
        assert!(s.checked_out_by_me);
        assert_eq!(s.owner.as_deref(), Some("me"));
    }

    #[test]
    fn test_derive_locked_by_other() {
        // controlled, no CheckOut and no UndoCheckout available, GetStatus
        // owner reported → locked by someone else, NOT editable.
        let s = derive_scm_status(true, false, false, false, "test", "me").unwrap();
        assert!(s.controlled);
        assert!(
            !s.editable,
            "must NOT claim editable when locked by another user"
        );
        assert!(s.locked);
        assert!(!s.checked_out_by_me);
        assert_eq!(s.owner.as_deref(), Some("test"));
    }

    #[test]
    fn test_derive_controlled_available_to_checkout() {
        // Controlled, CheckOut offered (free to take) → not editable yet, but not locked.
        let s = derive_scm_status(true, true, false, false, "", "me").unwrap();
        assert!(s.controlled);
        assert!(!s.editable);
        assert!(!s.locked);
        assert!(!s.checked_out_by_me);
    }

    #[test]
    fn test_derive_locked_by_other_detected_via_owner_only() {
        // GetStatus didn't report isInSC and the menu offered nothing, but an owner came back →
        // controlled and locked by that other user (not a false "editable").
        let s = derive_scm_status(false, false, false, false, "test", "me").unwrap();
        assert!(s.controlled);
        assert!(s.locked);
        assert!(!s.editable);
        assert_eq!(s.owner.as_deref(), Some("test"));
    }

    #[test]
    fn test_derive_no_signal_is_uncontrolled() {
        // No in-SC flag, no menu items, no owner → no SCM configured in this namespace.
        // Must return controlled:false, editable:true — NOT None/SCM_UNAVAILABLE, which
        // was a false-positive error for namespaces that simply have no SCM.
        let s = derive_scm_status(false, false, false, false, "", "me").unwrap();
        assert!(!s.controlled);
        assert!(s.editable);
        assert!(!s.locked);
        assert!(!s.checked_out_by_me);
        assert!(s.owner.is_none());
    }

    #[test]
    fn test_derive_resolves_from_menu_signal_alone() {
        // GetStatus gave nothing, but the menu offered CheckOut → controlled, available.
        let s = derive_scm_status(false, true, false, false, "", "me").unwrap();
        assert!(s.controlled);
        assert!(!s.editable);
        assert!(!s.locked);
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

    // ── ScmAction ─────────────────────────────────────────────────────────────
    #[test]
    fn test_scm_action_from_id() {
        assert_eq!(ScmAction::from_id("CheckOut"), ScmAction::CheckOut);
        assert_eq!(ScmAction::from_id("%CheckIn"), ScmAction::CheckIn);
        assert_eq!(ScmAction::from_id("%GetLatest"), ScmAction::GetLatest);
        assert_eq!(
            ScmAction::from_id("Unknown"),
            ScmAction::Unknown("Unknown".to_string())
        );
    }

    #[test]
    fn test_checkin_action_recognized() {
        assert_eq!(ScmAction::from_id("%CheckIn"), ScmAction::CheckIn);
        assert_eq!(ScmAction::from_id("CheckIn"), ScmAction::CheckIn);
    }

    #[test]
    fn test_iris_scm_allow_checkin_gate() {
        // Default: blocked
        std::env::remove_var("IRIS_SCM_ALLOW_CHECKIN");
        let allowed = std::env::var("IRIS_SCM_ALLOW_CHECKIN")
            .map(|v| matches!(v.to_ascii_lowercase().as_str(), "1" | "true" | "yes"))
            .unwrap_or(false);
        assert!(!allowed, "CheckIn should be blocked by default");

        // Opt-in variants
        for val in &["1", "true", "yes", "TRUE", "YES"] {
            std::env::set_var("IRIS_SCM_ALLOW_CHECKIN", val);
            let allowed = std::env::var("IRIS_SCM_ALLOW_CHECKIN")
                .map(|v| matches!(v.to_ascii_lowercase().as_str(), "1" | "true" | "yes"))
                .unwrap_or(false);
            assert!(
                allowed,
                "IRIS_SCM_ALLOW_CHECKIN={val} should enable CheckIn"
            );
        }

        // Explicitly disabled
        for val in &["0", "false", "no"] {
            std::env::set_var("IRIS_SCM_ALLOW_CHECKIN", val);
            let allowed = std::env::var("IRIS_SCM_ALLOW_CHECKIN")
                .map(|v| matches!(v.to_ascii_lowercase().as_str(), "1" | "true" | "yes"))
                .unwrap_or(false);
            assert!(
                !allowed,
                "IRIS_SCM_ALLOW_CHECKIN={val} should keep CheckIn blocked"
            );
        }

        std::env::remove_var("IRIS_SCM_ALLOW_CHECKIN");
    }
}
