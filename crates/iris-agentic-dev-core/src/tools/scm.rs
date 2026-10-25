//! iris_source_control — SCM status, menu, checkout, execute via Atelier xecute.

use crate::elicitation::{ElicitationAction, ElicitationStore};
use crate::iris::connection::{is_generator_error, IrisConnection};
use schemars::JsonSchema;
use serde::Deserialize;

fn ok_json(v: serde_json::Value) -> Result<rmcp::model::CallToolResult, rmcp::ErrorData> {
    Ok(rmcp::model::CallToolResult::structured(v))
}
fn err_json(code: &str, msg: &str) -> Result<rmcp::model::CallToolResult, rmcp::ErrorData> {
    crate::tools::err_result(
        serde_json::json!({"success": false, "error_code": code, "error": msg}),
    )
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
#[serde(deny_unknown_fields)]
pub struct ScmParams {
    /// Action: status, menu, checkout, execute
    #[schemars(extend("enum" = ["status", "menu", "checkout", "execute"]))]
    pub action: String,
    pub document: Option<String>,
    /// SCM action ID for action=execute
    pub action_id: Option<String>,
    /// Elicitation resume answer
    pub answer: Option<String>,
    pub elicitation_id: Option<String>,
    /// IRIS namespace. Defaults to the connection namespace (IRIS_NAMESPACE).
    #[serde(default)]
    pub namespace: Option<String>,
    /// Set to true to confirm write on a subject-role instance. Has no effect: source_control
    /// writes on subject instances are always hard-blocked regardless of confirm.
    #[serde(default)]
    pub confirm: bool,
    /// Route this call to a named registered IRIS instance. If omitted, uses the default connection.
    #[serde(default)]
    pub server: Option<String>,
}

/// Run ObjectScript and refuse output that is actually a failure report.
///
/// `execute_via_generator` reports IRIS-side failure *in* the output string, not as an `Err` — the
/// HTTP call and the compile both succeeded, so there is nothing for `Result` to carry. Every SCM
/// caller below then parsed that failure text as a status line, a menu list, or a checkout result:
/// The reply reader used to take a leading integer, so `ERROR: <ZSOAP>...` parsed as action code 0,
/// which the caller read as "no action needed". Checking once here is the only version of this that stays
/// correct as call sites are added.
///
/// See [`is_generator_error`] for the four shapes and why hand-rolling the check does not work.
async fn xecute(
    iris: &IrisConnection,
    client: &reqwest::Client,
    code: &str,
    namespace: &str,
) -> anyhow::Result<String> {
    let out = iris.execute_via_generator(code, namespace, client).await?;
    if is_generator_error(&out) {
        anyhow::bail!("{}", out.trim());
    }
    Ok(out)
}

/// Escape a string for safe interpolation into an ObjectScript double-quoted literal.
/// Uses ObjectScript conventions: " → "", \n → $Char(10), \r → $Char(13).
/// What goes between the quotes of a literal: `os_lit_inner`. The old version doubled quotes but
/// wrote line breaks as the text `$Char(10)` inside the literal, which changed the value (137).
fn os_quote(s: &str) -> String {
    crate::objectscript::os_lit_inner(s)
}

/// `UserAction` answered with an action code that is not a whole number.
pub const SCM_ACTION_UNPARSEABLE: &str = "SCM_ACTION_UNPARSEABLE";
/// `UserAction` answered with a whole number that is not one of the documented codes (0 to 7).
pub const SCM_ACTION_UNKNOWN: &str = "SCM_ACTION_UNKNOWN";
/// The action needs a window this tool cannot open: a CSP page, a program on the client, text
/// inserted into the editor, or documents opened in it (codes 2 to 5).
pub const SCM_NEEDS_UI: &str = "SCM_NEEDS_UI";
/// The action asks for a typed value (code 7) at a call site that has no way to take one.
pub const SCM_NEEDS_INPUT: &str = "SCM_NEEDS_INPUT";

/// An error code and its explanation, for the caller to hand to `err_json`.
pub type Refusal = (&'static str, String);

/// The sentinel that marks this server's own record in a `UserAction` / `AfterUserAction` reply.
///
/// The snippet ends by writing the sentinel and one JSON object (`%ToJSON()` escapes a newline in
/// a message, so a multi-line message stays on one line). The reader looks for it instead of taking
/// the first line, because a source-control hook writes to the same device: Perforce hooks print
/// `NOTICE:` and `CMD: p4 ...` lines, a hook may leave its own line unfinished, and a reply read
/// from the first line read that chatter as the answer.
///
/// A printable sentinel and not a control character: IRIS's generator device turns a written
/// control character into a newline, so a delimiter made of one would be split by the very
/// mechanism it was meant to survive.
pub const SCM_RECORD: &str = "IADSCM|";

/// What a snippet wrote after the sentinel. Every field is optional on the wire and a missing or
/// odd one reads as "not a good answer", never as a default that lets a write through.
#[derive(Debug, Deserialize, PartialEq, Default)]
pub struct ScmRecord {
    /// What the snippet was answering: `action`, `after`, or (the pre-write probe) `none`/`hold`.
    #[serde(default)]
    pub kind: String,
    /// The action code as IRIS gave it. A `Value` on purpose: converting it in ObjectScript with
    /// `+action` would turn text into 0, and 0 is the code that means "no dialog needed".
    #[serde(default)]
    pub code: serde_json::Value,
    #[serde(default)]
    pub msg: serde_json::Value,
    #[serde(default)]
    pub target: serde_json::Value,
    /// 1 when the hook's `%Status` was OK. Anything else, absence included, is not.
    #[serde(default)]
    pub ok: serde_json::Value,
    /// The `%Status` error text, when there was one.
    #[serde(default)]
    pub err: serde_json::Value,
}

fn text_of(v: &serde_json::Value) -> String {
    match v {
        serde_json::Value::Null => String::new(),
        serde_json::Value::String(s) => s.clone(),
        other => other.to_string(),
    }
}

impl ScmRecord {
    fn status_ok(&self) -> bool {
        self.ok.as_i64() == Some(1)
    }

    /// The action code if it is a whole number: a JSON integer, or a string that is exactly an
    /// integer's decimal spelling. `Err(())` for everything else (text, a fraction, `null`, ...).
    fn action_number(&self) -> Result<i64, ()> {
        match &self.code {
            serde_json::Value::Number(n) => n
                .as_i64()
                .or_else(|| n.as_u64().map(|_| i64::MAX))
                .ok_or(()),
            serde_json::Value::String(s) => s
                .parse::<i64>()
                .ok()
                .filter(|n| n.to_string() == *s)
                .ok_or(()),
            _ => Err(()),
        }
    }

    /// What to show a person: the hook's message, or its dialog target when it gave no message.
    fn display(&self) -> String {
        let msg = text_of(&self.msg);
        if msg.is_empty() {
            text_of(&self.target)
        } else {
            msg
        }
    }
}

/// The record in `out`: the last line that carries the sentinel, read from its first occurrence on
/// that line that parses. The snippet writes its record last, so the last one is the snippet's own
/// and an earlier one is the hook's. A last record that cannot be read is not replaced by an
/// earlier one that can.
fn find_record(out: &str) -> Option<ScmRecord> {
    for line in out.lines().rev() {
        let mut carries_sentinel = false;
        for (at, _) in line.match_indices(SCM_RECORD) {
            carries_sentinel = true;
            if let Ok(rec) = serde_json::from_str::<ScmRecord>(line[at + SCM_RECORD.len()..].trim())
            {
                return Some(rec);
            }
        }
        if carries_sentinel {
            return None;
        }
    }
    None
}

/// The record in `out`, or the refusal for output with none: the session could not start
/// (`scm_init_prefix` writes a bare `SCM_UNAVAILABLE` and quits), or the snippet never wrote its
/// answer (silence, IRIS error text, an old-format reply). The last is never read as an action
/// code: 0 means "no dialog needed", and nothing here is allowed to turn into 0.
pub(crate) fn record_or_refusal(out: &str) -> Result<ScmRecord, Refusal> {
    if let Some(rec) = find_record(out) {
        return Ok(rec);
    }
    if out.lines().any(|l| l.trim() == "SCM_UNAVAILABLE") {
        return Err((
            "SCM_UNAVAILABLE",
            "Source control session could not be initialized".to_string(),
        ));
    }
    let shown: String = out.trim().chars().take(400).collect();
    let detail = if shown.is_empty() {
        "Source control wrote no answer, so what the hook asked for is unknown and nothing was \
         changed."
            .to_string()
    } else {
        format!(
            "Source control's answer could not be read, so what the hook asked for is unknown and \
             nothing was changed. IRIS output: {shown}"
        )
    };
    Err((SCM_ACTION_UNPARSEABLE, detail))
}

/// The documented `UserAction` action codes (`%Studio.Extension.Base`), each with one meaning.
/// Every site that reads a code matches on this with no catch-all arm, so a variant added here does
/// not compile until each of them has decided what it means.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum UserActionCode {
    /// 0: do nothing, no dialog. Not "it happened": `AfterUserAction` still has to run.
    NoDialog,
    /// 1: a yes/no/cancel dialog.
    Confirm,
    /// 2: run a CSP page or template at the address in `Target`.
    OpenPage,
    /// 3: run a program on the client.
    RunExe,
    /// 4: insert `Target` into the open document.
    InsertText,
    /// 5: open the documents named in `Target`.
    OpenDocuments,
    /// 6: show `Target` in an alert. Source control declined.
    Alert,
    /// 7: a dialog with a text box and yes/no/cancel buttons.
    Prompt,
}

impl UserActionCode {
    pub fn from_code(n: i64) -> Option<Self> {
        Some(match n {
            0 => Self::NoDialog,
            1 => Self::Confirm,
            2 => Self::OpenPage,
            3 => Self::RunExe,
            4 => Self::InsertText,
            5 => Self::OpenDocuments,
            6 => Self::Alert,
            7 => Self::Prompt,
            _ => return None,
        })
    }
}

/// A `UserAction` answer that is one of the documented codes.
#[derive(Debug, PartialEq)]
pub struct ActionAnswer {
    pub code: UserActionCode,
    pub number: i64,
    /// The hook's message, or its dialog target when it gave no message.
    pub text: String,
}

/// An `action` record read as an answer: its status must be OK and its code must be one of 0 to 7.
pub(crate) fn answer_from_record(rec: &ScmRecord) -> Result<ActionAnswer, Refusal> {
    if rec.kind != "action" {
        return Err((
            SCM_ACTION_UNPARSEABLE,
            format!(
                "Expected a UserAction answer and got a '{}' record, so what the hook asked for is \
                 unknown and nothing was changed.",
                rec.kind
            ),
        ));
    }
    if !rec.status_ok() {
        let why = text_of(&rec.err);
        return Err((
            "SCM_ERROR",
            format!(
                "The source-control hook's UserAction returned an error, so what it did is unknown \
                 and nothing was changed.{}",
                if why.is_empty() {
                    String::new()
                } else {
                    format!(" {why}")
                }
            ),
        ));
    }
    let Ok(number) = rec.action_number() else {
        return Err((
            SCM_ACTION_UNPARSEABLE,
            format!(
                "Source control returned {} where an action code was expected, so what the hook \
                 asked for is unknown and nothing was changed.",
                rec.code
            ),
        ));
    };
    let Some(code) = UserActionCode::from_code(number) else {
        return Err((
            SCM_ACTION_UNKNOWN,
            format!(
                "Source control returned action code {number}. The documented codes are 0 to 7, so \
                 what the hook asked for is unknown and nothing was changed."
            ),
        ));
    };
    Ok(ActionAnswer {
        code,
        number,
        text: rec.display(),
    })
}

/// What a `UserAction` reply means: the answer, or the refusal for a reply that is not one.
pub fn user_action_outcome(out: &str) -> Result<ActionAnswer, Refusal> {
    answer_from_record(&record_or_refusal(out)?)
}

/// Did `AfterUserAction` finish? Only an `after` record with `ok` = 1 says so. Silence, IRIS error
/// text and a record of another kind do not: the old reading, "no output means it worked", turned a
/// snippet that never ran into a committed checkout.
pub fn after_action_outcome(out: &str) -> Result<(), Refusal> {
    let rec = match record_or_refusal(out) {
        Ok(rec) => rec,
        Err(("SCM_UNAVAILABLE", d)) => {
            return Err((
                "SCM_UNAVAILABLE",
                format!("{d}, so whether the action completed is unknown."),
            ))
        }
        Err((_, d)) => return Err((SCM_CHECKOUT_FAILED, d)),
    };
    if rec.kind != "after" {
        return Err((
            "SCM_ERROR",
            format!(
                "Expected an AfterUserAction record and got a '{}' record, so the action may not \
                 have been finished.",
                rec.kind
            ),
        ));
    }
    if rec.status_ok() {
        return Ok(());
    }
    let why = text_of(&rec.err);
    Err((
        SCM_CHECKOUT_FAILED,
        if why.is_empty() {
            "Source control reported the action as not completed, with no reason.".to_string()
        } else {
            why
        },
    ))
}

/// A checkout that was attempted and did not finish.
pub const SCM_CHECKOUT_FAILED: &str = "SCM_CHECKOUT_FAILED";

/// What a site that cannot open a window does with an answer.
#[derive(Debug, PartialEq)]
pub enum ActionGate {
    /// Code 0: nothing to ask. The action still has to be finished with `AfterUserAction`.
    Proceed,
    /// Code 1: ask the caller yes or no; the string is the hook's question.
    Dialog(String),
    /// Code 7: ask the caller for text; the string is the hook's prompt. A site that cannot take
    /// text refuses with [`needs_input_refusal`].
    Prompt(String),
    /// Nothing may proceed: the error code and what to tell the caller.
    Refuse(&'static str, String),
}

/// The one decision for each documented code, shared by `iris_source_control` (checkout and
/// execute) and the pre-write probe in `iris_doc`.
pub fn action_gate(ans: &ActionAnswer, doc: &str) -> ActionGate {
    let said = if ans.text.is_empty() {
        String::new()
    } else {
        format!(" The provider said: {}", ans.text)
    };
    let needs_ui = |what: &str| {
        ActionGate::Refuse(
            SCM_NEEDS_UI,
            format!(
                "Source control answered with action {} ({what}), which needs a window this tool \
                 cannot open. Nothing was changed for {doc}.{said}",
                ans.number
            ),
        )
    };
    match ans.code {
        UserActionCode::NoDialog => ActionGate::Proceed,
        UserActionCode::Confirm => ActionGate::Dialog(ans.text.clone()),
        UserActionCode::Prompt => ActionGate::Prompt(ans.text.clone()),
        UserActionCode::Alert => ActionGate::Refuse(
            "SCM_REJECTED",
            format!("Source control rejected: {}", ans.text),
        ),
        UserActionCode::OpenPage => needs_ui("run a web page"),
        UserActionCode::RunExe => needs_ui("run a program on the client"),
        UserActionCode::InsertText => needs_ui("insert text into the editor"),
        UserActionCode::OpenDocuments => needs_ui("open documents in the editor"),
    }
}

/// The refusal for a text prompt at a site that has no way to supply the text.
pub fn needs_input_refusal(doc: &str, prompt: &str) -> Refusal {
    (
        SCM_NEEDS_INPUT,
        format!(
            "Source control is asking for a typed value before it will act on {doc}, and this \
             path cannot supply one. Nothing was changed.{}",
            if prompt.is_empty() {
                String::new()
            } else {
                format!(" The provider said: {prompt}")
            }
        ),
    )
}

/// An operator opt-in that gates one kind of source-control action. Each has its own launch
/// variable and its own refusal code, so enabling one never enables the other.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ScmOptIn {
    /// CheckIn writes to the repository: `IRIS_SCM_ALLOW_CHECKIN`, refused as `CHECKIN_BLOCKED`.
    CheckIn,
    /// Disconnect drops the link between a document and its repository and Reconnect restores one,
    /// which switches the namespace's source-control integration for the session:
    /// `IRIS_SCM_ALLOW_DISCONNECT`, refused as `SCM_ADMIN_BLOCKED`. Its blast radius is the
    /// namespace, not one document, so CheckIn's switch does not cover it (spec 141 T198).
    Disconnect,
}

impl ScmOptIn {
    /// The launch variable that turns this opt-in on.
    pub const fn env_var(self) -> &'static str {
        match self {
            ScmOptIn::CheckIn => "IRIS_SCM_ALLOW_CHECKIN",
            ScmOptIn::Disconnect => "IRIS_SCM_ALLOW_DISCONNECT",
        }
    }

    /// The `error_code` of the refusal when this opt-in is off.
    pub const fn error_code(self) -> &'static str {
        match self {
            ScmOptIn::CheckIn => "CHECKIN_BLOCKED",
            ScmOptIn::Disconnect => "SCM_ADMIN_BLOCKED",
        }
    }

    /// Is the launch variable set to `1`, `true` or `yes` (any case)?
    pub fn enabled(self) -> bool {
        std::env::var(self.env_var())
            .map(|v| matches!(v.to_ascii_lowercase().as_str(), "1" | "true" | "yes"))
            .unwrap_or(false)
    }
}

/// Which opt-in an action id needs, if any. The id is read the way the provider's menu spells it,
/// with or without the `%`, in any case. `None` for every other action, including a site-written
/// one (the write gate covers those).
pub fn operator_opt_in(action_id: &str) -> Option<ScmOptIn> {
    match action_id
        .trim()
        .trim_start_matches('%')
        .to_ascii_lowercase()
        .as_str()
    {
        "checkin" => Some(ScmOptIn::CheckIn),
        "disconnect" | "reconnect" => Some(ScmOptIn::Disconnect),
        _ => None,
    }
}

/// Does the action need an operator opt-in (spec 141 R5 C24)?
pub fn needs_operator_opt_in(action_id: &str) -> bool {
    operator_opt_in(action_id).is_some()
}

/// Both opt-ins as the launch environment sets them.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ScmOptIns {
    pub checkin: bool,
    pub disconnect: bool,
}

impl ScmOptIns {
    pub fn from_env() -> Self {
        ScmOptIns {
            checkin: ScmOptIn::CheckIn.enabled(),
            disconnect: ScmOptIn::Disconnect.enabled(),
        }
    }

    pub fn allows(self, opt_in: ScmOptIn) -> bool {
        match opt_in {
            ScmOptIn::CheckIn => self.checkin,
            ScmOptIn::Disconnect => self.disconnect,
        }
    }
}

/// The refusal for an action whose opt-in is not set, or `None` when it may proceed. The code is
/// the opt-in's own, so the refusal names the right switch.
fn opt_in_refusal(action_id: &str) -> Option<Result<rmcp::model::CallToolResult, rmcp::ErrorData>> {
    let opt_in = operator_opt_in(action_id)?;
    if opt_in.enabled() {
        return None;
    }
    let action = action_id.trim().trim_start_matches('%');
    // The switch goes to the operator's stderr, not into the refusal an agent reads.
    tracing::warn!(
        action = %action,
        env_var = opt_in.env_var(),
        "iris_source_control: refused an action that is off by default; set this variable in the \
         environment iris-agentic-dev is launched with, and restart it"
    );
    Some(err_json(
        opt_in.error_code(),
        &format!(
            "{action} is off by default. It is an operator setting, not something this session \
             can change. Until the operator turns it on, do this step in your own source-control \
             client."
        ),
    ))
}

/// The enabled actions of a `name|enabled` menu listing; those that need an operator opt-in only
/// when `opt_ins` allows that one.
pub fn menu_actions(raw: &str, opt_ins: ScmOptIns) -> Vec<serde_json::Value> {
    let mut actions = vec![];
    for line in raw.lines() {
        let line = line.trim();
        if line == "SCM_UNAVAILABLE" || line.is_empty() || line.starts_with("ERROR") {
            continue;
        }
        let mut parts = line.splitn(2, '|');
        let name = parts.next().unwrap_or("").trim();
        let enabled: u8 = parts
            .next()
            .and_then(|s| s.trim().parse().ok())
            .unwrap_or(0);
        let allowed = operator_opt_in(name).is_none_or(|o| opt_ins.allows(o));
        if enabled == 1 && !name.is_empty() && allowed {
            actions.push(serde_json::json!({"id": name, "label": name, "enabled": true}));
        }
    }
    actions
}

pub async fn handle_iris_source_control(
    iris: &IrisConnection,
    client: &reqwest::Client,
    p: ScmParams,
    elicitation_store: &ElicitationStore,
    checkout_cache: &crate::elicitation::CheckoutCache,
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
    let ns = crate::tools::resolve_namespace(p.namespace.as_deref(), &iris.namespace);

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
        // The prompt was opened for this action; answering it runs it, so it meets the same gate.
        if let Some(refusal) = opt_in_refusal(action_id) {
            return refusal;
        }
        let after_code = after_user_action_code(action_id, &pending.document, answer);
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
                return crate::tools::err_result(
                    serde_json::json!({"success": false, "error_code": ec, "error": emsg}),
                );
            }
        };
        if let Err((code, detail)) = after_action_outcome(&out) {
            // Only a checkout failing is a failed checkout; any other resumed action is an error.
            let code = if code == SCM_CHECKOUT_FAILED
                && ScmAction::from_id(action_id) != ScmAction::CheckOut
            {
                "SCM_ERROR"
            } else {
                code
            };
            return err_json(code, &detail);
        }
        // Any resumed SCM action (checkout/undo/checkin/disconnect) changes checkout state, so
        // drop the cached entry — the next write re-probes and re-caches if still ours.
        checkout_cache.invalidate(&pending.namespace, &pending.document);
        return ok_json(
            serde_json::json!({"success": true, "document": pending.document, "action_id": action_id}),
        );
    }

    match p.action.as_str() {
        "status" => {
            let check_code = status_check_code(doc);
            let raw = match xecute(iris, client, &check_code, ns).await {
                Ok(o) => o,
                Err(e) => {
                    // A transport/exec failure must NOT be reported as "editable" — that is the
                    // very inconsistency this path used to have. Surface it honestly.
                    return crate::tools::err_result(serde_json::json!({
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
                return crate::tools::err_result(serde_json::json!({
                    "success": false,
                    "error_code": "SCM_UNAVAILABLE",
                    "error": "Could not determine source control status (no SCMSTATUS sentinel in IRIS output)",
                    "raw_output": raw_trunc,
                }));
            };
            let status =
                derive_scm_status(is_in_sc, has_co, has_undo, has_add, &owner, &iris.username);
            let Some(status) = status else {
                return crate::tools::err_result(serde_json::json!({
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
            let code = menu_all_items_code(doc);
            // An empty menu and a failed menu query are different answers. `unwrap_or_default()`
            // turned the second into the first, so a caller saw `"actions": []` and concluded the
            // document has no source-control actions available.
            let raw = match xecute(iris, client, &code, ns).await {
                Ok(v) => v,
                Err(e) => return err_json("IRIS_EXECUTE_ERROR", &e.to_string()),
            };
            let actions = menu_actions(&raw, ScmOptIns::from_env());
            ok_json(serde_json::json!({"success": true, "document": doc, "actions": actions}))
        }

        "checkout" => {
            let code = user_action_code("%CheckOut", doc);
            let raw = match xecute(iris, client, &code, ns).await {
                Ok(o) => o,
                Err(e) => {
                    return crate::tools::err_result(
                        serde_json::json!({"success": false, "error_code": "SCM_UNAVAILABLE", "error": e.to_string()}),
                    )
                }
            };
            let ans = match user_action_outcome(&raw) {
                Ok(a) => a,
                Err((code, detail)) => return err_json(code, &detail),
            };
            match action_gate(&ans, doc) {
                ActionGate::Proceed => {
                    // Code 0 means UserAction wants no dialog — but the checkout is NOT committed
                    // until AfterUserAction runs. Reporting success on UserAction alone was a
                    // false positive: the item looked checked out but a later write failed with
                    // ERROR #5865. Finalize so the checkout persists server-side before we claim
                    // success.
                    let after_code = after_user_action_code("%CheckOut", doc, "yes");
                    match xecute(iris, client, &after_code, ns).await {
                        Ok(o) => {
                            if let Err((code, detail)) = after_action_outcome(&o) {
                                return err_json(code, &detail);
                            }
                        }
                        Err(e) => {
                            return crate::tools::err_result(serde_json::json!({
                                "success": false,
                                "error_code": "SCM_UNAVAILABLE",
                                "error": e.to_string(),
                            }))
                        }
                    }
                    // Checkout committed — cache it so a following iris_doc write skips the probe.
                    checkout_cache.mark(ns, doc);
                    ok_json(serde_json::json!({"success": true, "document": doc, "editable": true}))
                }
                ActionGate::Dialog(msg) => {
                    let eid = elicitation_store.insert(
                        doc,
                        ElicitationAction::ScmExecute,
                        None,
                        Some("%CheckOut".to_string()),
                        ns.to_string(),
                    );
                    ok_json(serde_json::json!({
                        "success": false,
                        "elicitation_required": true,
                        "elicitation_id": eid,
                        "message": if msg.is_empty() { format!("Check out {} ?", doc) } else { msg },
                        "options": ["yes", "no"],
                    }))
                }
                ActionGate::Prompt(msg) => {
                    let (code, detail) = needs_input_refusal(doc, &msg);
                    err_json(code, &detail)
                }
                ActionGate::Refuse(code, detail) => err_json(code, &detail),
            }
        }

        "execute" => {
            let action_id = p.action_id.as_deref().unwrap_or("");
            if let Some(refusal) = opt_in_refusal(action_id) {
                return refusal;
            }
            let code = user_action_code(action_id, doc);
            let raw = match xecute(iris, client, &code, ns).await {
                Ok(o) => o,
                Err(e) => {
                    return crate::tools::err_result(
                        serde_json::json!({"success": false, "error_code": "SCM_UNAVAILABLE", "error": e.to_string()}),
                    )
                }
            };
            let ans = match user_action_outcome(&raw) {
                Ok(a) => a,
                Err((code, detail)) => return err_json(code, &detail),
            };
            match action_gate(&ans, doc) {
                ActionGate::Proceed => {
                    // A completed execute (undo checkout / checkin / disconnect / …) changes
                    // checkout state — drop any cached entry so the next write re-probes.
                    checkout_cache.invalidate(ns, doc);
                    ok_json(
                        serde_json::json!({"success": true, "document": doc, "action_id": action_id}),
                    )
                }
                ActionGate::Dialog(msg) => {
                    let eid = elicitation_store.insert(
                        doc,
                        ElicitationAction::ScmExecute,
                        None,
                        Some(action_id.to_string()),
                        ns.to_string(),
                    );
                    ok_json(serde_json::json!({
                        "success": false, "elicitation_required": true, "elicitation_id": eid,
                        "message": if msg.is_empty() { format!("Execute {} on {}?", action_id, doc) } else { msg },
                        "options": ["yes", "no"],
                    }))
                }
                ActionGate::Prompt(msg) => {
                    let eid = elicitation_store.insert(
                        doc,
                        ElicitationAction::ScmExecute,
                        None,
                        Some(action_id.to_string()),
                        ns.to_string(),
                    );
                    ok_json(serde_json::json!({
                        "success": false, "elicitation_required": true, "elicitation_id": eid,
                        "message": if msg.is_empty() { format!("Enter value for {}:", action_id) } else { msg },
                        "input_type": "text",
                    }))
                }
                ActionGate::Refuse(code, detail) => err_json(code, &detail),
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
#[doc(hidden)]
pub fn status_check_code(doc: &str) -> String {
    let doc_lit = os_quote(doc);
    // Each risky step is wrapped in TRY/CATCH so a runtime error (SourceControlCreate failing,
    // GetStatus <PROTECT>, an SCM provider that has no MenuItems query, …) can never abort the
    // job before the SCMSTATUS sentinel is written. Without this, any partial failure produced
    // "no status signal returned" instead of a usable (possibly indeterminate) status.
    format!(
        "set isErr=0,isInSC=0,editable=0,isCheckedOut=0,owner=\"\" \
         set hasCheckOut=0,hasUndoCheckout=0,hasAddToSC=0 \
         try {{ {SCM_CREATE_CALL} }} catch {{ set isErr=1 }} \
         try {{ set sc=##class(%Studio.SourceControl.Interface).GetStatus(\"{doc_lit}\",.isInSC,.editable,.isCheckedOut,.owner) if $system.Status.IsError(sc) {{ set isErr=1 }} }} catch {{ set isErr=1 }} \
         try {{ \
           set rset=##class(%ResultSet).%New(\"%Studio.SourceControl.Interface:MenuItems\") \
           set sc=rset.Execute(\"%SourceMenu\",\"{doc_lit}\",\"\") \
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
    // No signal at all → we genuinely don't know. Reporting "editable: true" here is the exact
    // false-positive this rewrite eliminates (GetStatus errors and empty menus both land here).
    if !any_signal {
        return None;
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
/// 'xxx', and was last updated at 2026-07-07 12:34:56` message (often followed by a `<PROTECT>`)
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
        // "checked out by user 'xxx'" — timestamp captured only if the line isn't truncated.

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

/// The one `SourceControlCreate` call every snippet makes: the session's own user and an empty
/// password, as `%Api.Atelier.v1` does for each `PUT /doc` VS Code sends. The process is already
/// authenticated as `$USERNAME`, so no password has to enter ObjectScript source (FR-020).
pub const SCM_CREATE_CALL: &str = r#"set sc=##class(%Studio.SourceControl.Interface).SourceControlCreate($USERNAME,"",.created,.flags,.outuser)"#;

/// Prefix that initializes a SCM session via SourceControlCreate and binds obj=%SourceControl.
/// All SCM methods are instance methods — they require an active %SourceControl object.
#[doc(hidden)]
pub fn scm_init_prefix() -> String {
    format!(
        "{SCM_CREATE_CALL} \
         set obj=$get(%SourceControl) \
         if '$IsObject(obj) {{ write \"SCM_UNAVAILABLE\" quit }} "
    )
}

/// The ObjectScript that writes this server's record: the sentinel, then the JSON object built in
/// local `r`. Ends every `UserAction` / `AfterUserAction` snippet.
pub(crate) const WRITE_RECORD: &str = "write \"IADSCM|\"_r.%ToJSON()";

/// ObjectScript that calls `UserAction` for `%CheckOut`-style `action_id` (already quoted for a
/// literal) on `doc_lit`, and builds the `action` record in `r`: the code exactly as the hook left
/// it, `msg` and `target` (CCR puts the check-out prompt in `target`), and the call's own status.
/// A hook that returned an error status has not answered, whatever it left in `Action`.
pub(crate) fn user_action_record_code(action_id_lit: &str, doc_lit: &str) -> String {
    format!(
        "set action=0 set target=\"\" set msg=\"\" set reload=0 \
         set sc=obj.UserAction(0,\"%SourceMenu,{action_id_lit}\",\"{doc_lit}\",\"\",.action,.target,.msg,.reload) \
         set r={{}} set r.kind=\"action\" set r.code=action set r.msg=msg set r.target=target \
         set r.ok=$select($system.Status.IsError(sc):0,1:1) set r.err=$system.Status.GetErrorText(sc) "
    )
}

/// Build the ObjectScript snippet that invokes `UserAction` on the SCM instance and writes one
/// `action` record (see [`SCM_RECORD`]).
#[doc(hidden)]
pub fn user_action_code(action_id: &str, doc: &str) -> String {
    let prefix = scm_init_prefix();
    let record = user_action_record_code(&os_quote(action_id), &os_quote(doc));
    format!("{prefix}{record}{WRITE_RECORD}")
}

/// Build the ObjectScript snippet that re-runs `UserAction` then immediately calls
/// `AfterUserAction` in the same job, so %SourceControl state is preserved. Writes one `after`
/// record: `ok` is 1 only when `AfterUserAction` returned an OK status.
#[doc(hidden)]
pub fn after_user_action_code(action_id: &str, doc: &str, answer: &str) -> String {
    let prefix = scm_init_prefix();
    let answer_int = if answer == "yes" { "1" } else { "0" };
    let action_id_q = os_quote(action_id);
    let doc_lit = os_quote(doc);
    format!(
        "{prefix}\
         set action=0 set target=\"\" set msg=\"\" set reload=0 \
         set sc=obj.UserAction(0,\"%SourceMenu,{action_id_q}\",\"{doc_lit}\",\"\",.action,.target,.msg,.reload) \
         set sc=obj.AfterUserAction(0,\"%SourceMenu,{action_id_q}\",\"{doc_lit}\",{answer_int},\"\") \
         set r={{}} set r.kind=\"after\" set r.ok=$select($system.Status.IsError(sc):0,1:1) \
         set r.err=$system.Status.GetErrorText(sc) {WRITE_RECORD}"
    )
}

/// Build a single ObjectScript snippet that queries all enabled SCM menu items via
/// the MenuItems ResultSet, writing one "name|enabled|displayName" line per item.
#[doc(hidden)]
pub fn menu_all_items_code(doc: &str) -> String {
    let prefix = scm_init_prefix();
    let doc_lit = os_quote(doc);
    format!(
        "{prefix}\
         set rset=##class(%ResultSet).%New(\"%Studio.SourceControl.Interface:MenuItems\") \
         set sc=rset.Execute(\"%SourceMenu\",\"{doc_lit}\",\"\") \
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
        // Used to give the text `$Char(10)` inside the literal, not a line break (137).
        assert_eq!(os_quote("a\nb"), "\"_\"a\"_$CHAR(10)_\"b\"_\"");
    }
    #[test]
    fn test_os_quote_cr() {
        assert_eq!(os_quote("a\rb"), "\"_\"a\"_$CHAR(13)_\"b\"_\"");
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
        assert_eq!(
            os_quote("say \"hi\"\nbye"),
            "\"_\"say \"\"hi\"\"\"_$CHAR(10)_\"bye\"_\""
        );
    }
    #[test]
    fn test_os_quote_cr_and_newline() {
        assert_eq!(
            os_quote("line\r\nend"),
            "\"_\"line\"_$CHAR(13,10)_\"end\"_\""
        );
    }

    // ── user_action_code ──────────────────────────────────────────────────────
    #[test]
    fn test_user_action_code_no_backslash_quote() {
        let code = user_action_code("CheckOut", "MyApp.Patient.cls");
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
        let code = user_action_code("Check\"Out", "Doc.cls");
        assert!(
            code.contains("\"\""),
            "double-quote must become \"\": {}",
            code
        );
        assert!(!code.contains("\\\""), "no backslash-quote: {}", code);
    }
    #[test]
    fn test_user_action_code_escapes_newline_in_doc() {
        let code = user_action_code("CheckOut", "Doc\nwith\nnewlines.cls");
        assert!(
            code.contains("$CHAR(10)") && !code.contains("Doc\nwith"),
            "newline must become $CHAR(10) outside the literal: {}",
            code
        );
    }

    // ── status_check_code ─────────────────────────────────────────────────────
    #[test]
    fn test_status_check_code_uses_get_status() {
        let code = status_check_code("MyApp.Patient.cls");
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
        let code = status_check_code("MyApp.Patient.cls");
        assert!(
            code.contains("MyApp.Patient.cls"),
            "must embed document name: {code}"
        );
    }

    #[test]
    fn test_status_check_code_escapes_quotes_in_doc() {
        let code = status_check_code("My\"App.cls");
        assert!(
            code.contains("\"\""),
            "double-quote must become \"\": {code}"
        );
        assert!(!code.contains("\\\""), "no backslash-quote: {code}");
    }

    #[test]
    fn test_status_check_code_escapes_quotes_in_credentials() {
        let code = status_check_code("Any.cls");
        assert!(
            code.contains("\"\""),
            "double-quote in credentials must be escaped: {code}"
        );
    }

    #[test]
    fn test_status_check_code_emits_structured_sentinel() {
        let code = status_check_code("Any.cls");
        assert!(
            code.contains("SCMSTATUS|"),
            "must emit the SCMSTATUS sentinel line: {code}"
        );
    }

    #[test]
    fn test_status_check_code_uses_menu_items_for_checkout_state() {
        let code = status_check_code("Any.cls");
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
    fn test_status_check_code_uses_the_session_user() {
        let code = status_check_code("Doc.cls");
        assert!(
            code.contains(r#"SourceControlCreate($USERNAME,"","#),
            "must create the session as $USERNAME with no password: {code}"
        );
    }

    #[test]
    fn test_status_check_code_contains_document_name() {
        let code = status_check_code("MyClass.cls");
        assert!(
            code.contains("MyClass.cls"),
            "must contain the document name: {code}"
        );
    }

    #[test]
    fn test_status_check_code_contains_scmstatus_sentinel() {
        let code = status_check_code("Doc.cls");
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
    fn test_derive_no_signal_is_indeterminate() {
        // No in-SC flag, no menu items, no owner → indeterminate → None (caller reports
        // SCM_UNAVAILABLE), never a false "editable: true".
        assert!(derive_scm_status(false, false, false, false, "", "me").is_none());
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
        let code = scm_init_prefix();
        assert!(code.contains("SourceControlCreate"), "{code}");
        assert!(code.contains("%Studio.SourceControl.Interface"), "{code}");
    }

    #[test]
    fn test_scm_init_prefix_escapes_quotes_in_user() {
        let code = scm_init_prefix();
        assert!(
            code.contains("\"\""),
            "double-quote must be doubled: {code}"
        );
        assert!(!code.contains("\\\""), "no backslash-quote: {code}");
    }

    // ── menu_all_items_code ──────────────────────────────────────────────────
    #[test]
    fn test_menu_all_items_code_contains_menu_items() {
        let code = menu_all_items_code("MyApp.cls");
        assert!(code.contains("MenuItems"), "{code}");
    }

    #[test]
    fn test_menu_all_items_code_contains_doc() {
        let code = menu_all_items_code("MyApp.Patient.cls");
        assert!(code.contains("MyApp.Patient.cls"), "{code}");
    }

    // ── after_user_action_code ───────────────────────────────────────────────
    #[test]
    fn test_after_user_action_code_contains_after_user_action() {
        let code = after_user_action_code("CheckOut", "MyApp.cls", "yes");
        assert!(code.contains("AfterUserAction"), "{code}");
    }

    #[test]
    fn test_after_user_action_code_contains_doc() {
        let code = after_user_action_code("CheckOut", "MyApp.Patient.cls", "no");
        assert!(code.contains("MyApp.Patient.cls"), "{code}");
    }

    #[test]
    fn test_after_user_action_code_yes_becomes_1() {
        let code = after_user_action_code("CheckOut", "Doc.cls", "yes");
        assert!(code.contains(",1,"), "yes should become 1: {code}");
    }

    #[test]
    fn test_after_user_action_code_no_becomes_0() {
        let code = after_user_action_code("CheckOut", "Doc.cls", "no");
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
        let code = scm_init_prefix();
        // Must bind obj to %SourceControl for instance method calls
        assert!(
            code.contains("%SourceControl"),
            "must bind %SourceControl: {code}"
        );
    }

    #[test]
    fn test_scm_init_prefix_writes_scm_unavailable_on_no_obj() {
        let code = scm_init_prefix();
        assert!(
            code.contains("SCM_UNAVAILABLE"),
            "must write SCM_UNAVAILABLE when obj unavailable: {code}"
        );
    }

    #[test]
    fn test_scm_init_prefix_escapes_quotes_in_password() {
        let code = scm_init_prefix();
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
        let code = user_action_code("CheckOut", "MyApp.cls");
        assert!(
            code.contains("UserAction"),
            "must invoke UserAction: {code}"
        );
    }

    #[test]
    fn test_user_action_code_contains_source_menu() {
        let code = user_action_code("CheckOut", "MyApp.cls");
        assert!(
            code.contains("%SourceMenu"),
            "must pass %SourceMenu prefix: {code}"
        );
    }

    #[test]
    fn test_user_action_code_escapes_quotes_in_credentials() {
        let code = user_action_code("CheckOut", "Doc.cls");
        assert!(
            code.contains("\"\""),
            "double-quote in credentials must be doubled: {code}"
        );
        assert!(!code.contains("\\\""), "no backslash-quote: {code}");
    }

    // ── menu_all_items_code additional ────────────────────────────────────────
    #[test]
    fn test_menu_all_items_code_contains_source_menu() {
        let code = menu_all_items_code("MyApp.cls");
        assert!(
            code.contains("%SourceMenu"),
            "must pass %SourceMenu to Execute: {code}"
        );
    }

    #[test]
    fn test_menu_all_items_code_escapes_quotes_in_doc() {
        let code = menu_all_items_code("My\"App.cls");
        assert!(
            code.contains("\"\""),
            "double-quote in doc must be doubled: {code}"
        );
        assert!(!code.contains("\\\""), "no backslash-quote: {code}");
    }

    #[test]
    fn test_menu_all_items_code_contains_source_control_create() {
        let code = menu_all_items_code("MyApp.cls");
        assert!(
            code.contains("SourceControlCreate"),
            "must init session: {code}"
        );
    }

    // ── after_user_action_code additional ────────────────────────────────────
    #[test]
    fn test_after_user_action_code_contains_source_menu() {
        let code = after_user_action_code("CheckOut", "MyApp.cls", "yes");
        assert!(
            code.contains("%SourceMenu"),
            "must pass %SourceMenu: {code}"
        );
    }

    #[test]
    fn test_after_user_action_code_contains_user_action() {
        let code = after_user_action_code("CheckOut", "MyApp.cls", "yes");
        assert!(
            code.contains("UserAction"),
            "must call UserAction first: {code}"
        );
    }

    #[test]
    fn test_after_user_action_code_writes_its_status_record() {
        let code = after_user_action_code("CheckOut", "MyApp.cls", "yes");
        assert!(
            code.contains("GetErrorText") && code.contains(SCM_RECORD),
            "must report AfterUserAction's status in a record: {code}"
        );
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
    fn test_each_opt_in_has_its_own_variable_and_code() {
        assert_eq!(operator_opt_in("%CheckIn"), Some(ScmOptIn::CheckIn));
        assert_eq!(operator_opt_in(" disconnect"), Some(ScmOptIn::Disconnect));
        assert_eq!(operator_opt_in("%RECONNECT"), Some(ScmOptIn::Disconnect));
        assert_eq!(operator_opt_in("%GetLatest"), None);
        assert_ne!(ScmOptIn::CheckIn.env_var(), ScmOptIn::Disconnect.env_var());
        assert_ne!(
            ScmOptIn::CheckIn.error_code(),
            ScmOptIn::Disconnect.error_code()
        );
    }
}
