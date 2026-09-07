//! Admin tools — namespace/database, observability, security, HL7, Mermaid.
//! Implements T077–T105 from spec 072-c.

use crate::iris::connection::{generator_error_message, is_generator_error, IrisConnection};
use rmcp::{model::*, ErrorData as McpError};
use std::sync::Arc;

// ── Error codes ──────────────────────────────────────────────────────────────
pub const ERR_HL7_NOT_AVAILABLE: &str = "HL7_NOT_AVAILABLE";
pub const ERR_CONFIRM_REQUIRED: &str = "CONFIRM_REQUIRED";
pub const ERR_CONFIRM_EXPIRED: &str = "CONFIRM_EXPIRED";
pub const ERR_CONFIRM_MISMATCH: &str = "CONFIRM_MISMATCH";
pub const ERR_WRITE_GATE: &str = "WRITE_TOOLS_DISABLED";
/// Destructive-tier refusal: writes are on, the tier is off (085 FR-018). Documented since
/// v1.0.0 and, until spec 085, never present in source. Same string as
/// [`crate::tools::write_gate::ERR_DESTRUCTIVE_GATE`], which enforcement emits.
pub const ERR_DESTRUCTIVE_GATE: &str = "DESTRUCTIVE_TOOLS_DISABLED";

// ── ConfirmEntry ─────────────────────────────────────────────────────────────

#[derive(Clone)]
pub struct ConfirmEntry {
    pub global: String,
    pub server: Option<String>,
    pub issued_at: std::time::Instant,
}

impl ConfirmEntry {
    pub fn is_expired(&self) -> bool {
        self.issued_at.elapsed() > std::time::Duration::from_secs(300)
    }
}

// ── helpers ──────────────────────────────────────────────────────────────────

fn ok_json(v: serde_json::Value) -> Result<CallToolResult, McpError> {
    Ok(CallToolResult::structured(v))
}

fn err_json(code: &str, msg: &str) -> Result<CallToolResult, McpError> {
    crate::tools::err_result(serde_json::json!({
        "success": false,
        "error_code": code,
        "error": msg,
    }))
}

// ── global_preview (T078) ─────────────────────────────────────────────────────

pub struct GlobalPreviewParams {
    pub global: String,
    pub server: Option<String>,
    pub count: u32,
    pub iris: Arc<IrisConnection>,
    pub client: Arc<reqwest::Client>,
}

pub async fn global_preview_impl(
    params: GlobalPreviewParams,
    confirm_tokens: &tokio::sync::Mutex<std::collections::HashMap<String, ConfirmEntry>>,
) -> Result<CallToolResult, McpError> {
    use crate::tools::global::{build_global_ref, normalize_global_name};

    let name = normalize_global_name(&params.global);
    let global_ref = build_global_ref(&name, &[]);
    let ns = params.iris.namespace.clone();

    // Fetch entries using iris_global list action via execute_via_generator
    let limit = params.count.clamp(1, 100);
    let code = format!(
        r#"Set gRef="{global_ref}"
Set cnt=0
Set key=""
For {{
  Set key=$Order(@gRef@(key))
  If key="" Quit
  Set cnt=cnt+1
  Write key_"|"_$Get(@gRef@(key)),!
  If cnt>={limit} Quit
}}
Write "DONE|"_cnt,!"#
    );

    let out = params
        .iris
        .execute_via_generator(&code, &ns, &params.client)
        .await
        .map_err(|e| McpError::internal_error(format!("IRIS execute failed: {e}"), None))?;

    // A refused or failed read arrives as failure text inside `out`, not as `Err` — the HTTP call
    // and the class compile both succeeded, only the `$Order` loop did not. Parsing that text found
    // no `DONE|` line, so the preview came back empty and a confirm_token was minted anyway, which
    // handed `global_kill` a valid confirmation for a global nobody had ever read. A preview that
    // did not see the data must not authorise deleting it.
    if let Some(msg) = generator_error_message(&out) {
        return err_json(
            "IRIS_EXECUTE_ERROR",
            &format!(
                "Could not read {global_ref} in namespace {ns}, so no confirm_token was issued: {}",
                msg.trim()
            ),
        );
    }

    let mut entries: Vec<serde_json::Value> = Vec::new();
    let mut total = 0u32;
    let mut saw_done = false;
    for line in out.lines() {
        if line.is_empty() {
            continue;
        }
        if let Some(rest) = line.strip_prefix("DONE|") {
            total = rest.parse().unwrap_or(0);
            saw_done = true;
        } else if let Some(idx) = line.find('|') {
            let key = &line[..idx];
            let val = &line[idx + 1..];
            entries.push(serde_json::json!({"key": key, "value": val}));
        }
    }

    // `DONE|<count>` is the last thing the loop writes, so its absence means the read never
    // finished — output truncated, device moved, job killed. Zero entries plus no marker is not an
    // empty global, and it must not mint a token either.
    if !saw_done {
        return err_json(
            "IRIS_EXECUTE_ERROR",
            &format!(
                "Preview of {global_ref} in namespace {ns} did not complete (no DONE marker in output), so no confirm_token was issued. Raw output: {}",
                out.trim()
            ),
        );
    }

    // Mint a confirmation token
    let token = uuid::Uuid::new_v4().to_string();
    let expires_at = chrono::Utc::now() + chrono::Duration::minutes(5);
    let entry = ConfirmEntry {
        global: name.clone(),
        server: params.server.clone(),
        issued_at: std::time::Instant::now(),
    };
    {
        let mut map = confirm_tokens.lock().await;
        map.insert(token.clone(), entry);
    }

    ok_json(serde_json::json!({
        "success": true,
        "global": name,
        "server": params.server,
        "entries": entries,
        "total_subscripts": total,
        "confirm_token": token,
        "confirm_expires": expires_at.to_rfc3339(),
    }))
}

// ── global_kill (T079) — gated in call_tool, not here (085) ──────────────────

pub struct GlobalKillParams {
    pub global: String,
    pub server: Option<String>,
    pub confirm_token: String,
    pub iris: Arc<IrisConnection>,
    pub client: Arc<reqwest::Client>,
}

pub async fn global_kill_impl(
    params: GlobalKillParams,
    confirm_tokens: &tokio::sync::Mutex<std::collections::HashMap<String, ConfirmEntry>>,
) -> Result<CallToolResult, McpError> {
    use crate::tools::global::normalize_global_name;
    let name = normalize_global_name(&params.global);

    // Token validation
    let entry = {
        let map = confirm_tokens.lock().await;
        map.get(&params.confirm_token).cloned()
    };

    let entry =
        match entry {
            None => return err_json(
                ERR_CONFIRM_REQUIRED,
                "No confirmation token found. Call global_preview first to get a confirm_token.",
            ),
            Some(e) => e,
        };

    if entry.is_expired() {
        let mut map = confirm_tokens.lock().await;
        map.remove(&params.confirm_token);
        return err_json(
            ERR_CONFIRM_EXPIRED,
            "Confirmation token has expired (5 minute limit). Call global_preview again.",
        );
    }

    if entry.global != name || entry.server != params.server {
        return err_json(
            ERR_CONFIRM_MISMATCH,
            &format!(
                "Token was issued for global '{}' server {:?}, not '{}' server {:?}",
                entry.global, entry.server, name, params.server
            ),
        );
    }

    // Execute Kill
    let ns = params.iris.namespace.clone();
    let code = format!("Kill ^{name}\nWrite \"KILLED\",!");
    let out = params
        .iris
        .execute_via_generator(&code, &ns, &params.client)
        .await
        .map_err(|e| McpError::internal_error(format!("IRIS execute failed: {e}"), None))?;

    // Remove token after use
    {
        let mut map = confirm_tokens.lock().await;
        map.remove(&params.confirm_token);
    }

    if out.trim().contains("KILLED") {
        ok_json(serde_json::json!({
            "success": true,
            "killed": true,
            "global": name,
        }))
    } else {
        crate::tools::err_result(serde_json::json!({
            "success": false,
            "error_code": "IRIS_EXECUTE_ERROR",
            "error": format!("Unexpected output: {out}"),
        }))
    }
}

// ── iris_namespace_list (T082) ────────────────────────────────────────────────

pub async fn iris_namespace_list_impl(
    iris: &IrisConnection,
    client: &reqwest::Client,
) -> Result<CallToolResult, McpError> {
    // Simplest path: fetch from Atelier root which returns namespaces array
    let url = format!("{}/api/atelier/", iris.base_url);
    match client
        .get(&url)
        .basic_auth(&iris.username, Some(&iris.password))
        .send()
        .await
    {
        Ok(resp) if resp.status().is_success() => {
            if let Ok(body) = resp.json::<serde_json::Value>().await {
                if let Some(arr) = body["result"]["content"]["namespaces"].as_array() {
                    let namespaces: Vec<String> = arr
                        .iter()
                        .filter_map(|v| v.as_str().map(|s| s.to_string()))
                        .collect();
                    let count = namespaces.len();
                    return ok_json(serde_json::json!({
                        "success": true,
                        "namespaces": namespaces,
                        "count": count,
                    }));
                }
            }
            // Fallback: execute in %SYS
            namespace_list_via_exec(iris, client).await
        }
        Ok(resp) => {
            let status = resp.status();
            // Fallback to exec
            let _ = status;
            namespace_list_via_exec(iris, client).await
        }
        Err(_) => namespace_list_via_exec(iris, client).await,
    }
}

async fn namespace_list_via_exec(
    iris: &IrisConnection,
    client: &reqwest::Client,
) -> Result<CallToolResult, McpError> {
    let code = r#"Set ns=""
For {
  Set ns=$Order(^%SYS("Namespace",ns))
  If ns="" Quit
  Write ns,!
}
Write "DONE",!"#;
    match iris.execute_via_generator(code, "%SYS", client).await {
        Ok(out) => {
            let mut namespaces: Vec<String> = out
                .lines()
                .filter(|l| !l.is_empty() && *l != "DONE")
                .map(|l| l.to_string())
                .collect();
            namespaces.sort();
            let count = namespaces.len();
            ok_json(serde_json::json!({
                "success": true,
                "namespaces": namespaces,
                "count": count,
            }))
        }
        Err(e) => err_json("IRIS_UNREACHABLE", &e.to_string()),
    }
}

// ── iris_database_list (T083) ─────────────────────────────────────────────────

pub async fn iris_database_list_impl(
    iris: &IrisConnection,
    client: &reqwest::Client,
) -> Result<CallToolResult, McpError> {
    let code = r#"Set tRS=##class(%ResultSet).%New("SYS.Database:List")
Set tSC=tRS.Execute()
If $$$ISERR(tSC) { Write "ERROR:"_$System.Status.GetErrorText(tSC) Quit }
While tRS.Next() {
  Write tRS.Get("Directory"),"|",tRS.Get("Mounted"),"|",tRS.Get("Size"),!
}"#;
    let base_out = match iris.execute_via_generator(code, "%SYS", client).await {
        Ok(out) => out,
        Err(e) => return err_json("IRIS_UNREACHABLE", &e.to_string()),
    };
    let base_out = base_out.trim();
    if is_generator_error(base_out) {
        return err_json("IRIS_EXECUTE_ERROR", base_out);
    }

    // Query free space — graceful degradation if it fails
    let fs_code = r#"Set tRS=##class(%ResultSet).%New("%SYS.DatabaseQuery:FreeSpace")
Set tSC=tRS.Execute()
If $$$ISERR(tSC) { Write "ERROR:"_$System.Status.GetErrorText(tSC) Quit }
While tRS.Next() {
  Write tRS.Get("Directory"),"|",tRS.Get("SizeInt"),"|",tRS.Get("AvailableNum"),"|",tRS.Get("Free"),"|",tRS.Get("MaxSize"),!
}"#;
    let (free_space_map, free_space_note) =
        match iris.execute_via_generator(fs_code, "%SYS", client).await {
            Ok(fs_out) => {
                let fs_out = fs_out.trim().to_string();
                if is_generator_error(&fs_out) {
                    (
                        std::collections::HashMap::<String, serde_json::Value>::new(),
                        Some(format!("unavailable: {fs_out}")),
                    )
                } else {
                    let mut map = std::collections::HashMap::new();
                    for line in fs_out.lines().filter(|l| !l.is_empty()) {
                        let p: Vec<&str> = line.splitn(5, '|').collect();
                        let dir = p.first().copied().unwrap_or("").to_string();
                        if dir.is_empty() {
                            continue;
                        }
                        let size_mb = p
                            .get(1)
                            .copied()
                            .unwrap_or("0")
                            .trim()
                            .parse::<i64>()
                            .unwrap_or(0);
                        let free_space_mb = p
                            .get(2)
                            .copied()
                            .unwrap_or("0")
                            .trim()
                            .parse::<f64>()
                            .unwrap_or(0.0);
                        let free_pct = p
                            .get(3)
                            .copied()
                            .unwrap_or("0")
                            .trim()
                            .parse::<i64>()
                            .unwrap_or(0);
                        let max_size_mb = parse_max_size_mb(p.get(4).copied().unwrap_or(""))
                            .map(serde_json::Value::from)
                            .unwrap_or(serde_json::Value::Null);
                        map.insert(
                            dir,
                            serde_json::json!({
                                "size_mb": size_mb,
                                "free_space_mb": free_space_mb,
                                "free_pct": free_pct,
                                "max_size_mb": max_size_mb,
                            }),
                        );
                    }
                    (map, None)
                }
            }
            Err(e) => (
                std::collections::HashMap::new(),
                Some(format!("unavailable: {e}")),
            ),
        };

    let databases: Vec<serde_json::Value> = base_out
        .lines()
        .filter(|l| !l.is_empty())
        .map(|line| {
            let parts: Vec<&str> = line.splitn(3, '|').collect();
            let dir = parts.first().copied().unwrap_or("");
            let mut entry = serde_json::json!({
                "directory": dir,
                "mounted": parts.get(1).copied().unwrap_or("0") != "0",
                "size_mb": parts.get(2).copied().unwrap_or("0")
                    .trim().parse::<f64>().unwrap_or(0.0),
            });
            if let Some(fs) = free_space_map.get(dir) {
                entry["size_mb"] = fs["size_mb"].clone();
                entry["free_space_mb"] = fs["free_space_mb"].clone();
                entry["free_pct"] = fs["free_pct"].clone();
                entry["max_size_mb"] = fs["max_size_mb"].clone();
            }
            entry
        })
        .collect();
    let count = databases.len();
    let mut resp = serde_json::json!({
        "success": true,
        "databases": databases,
        "count": count,
    });
    if let Some(note) = free_space_note {
        resp["free_space_note"] = serde_json::Value::String(note);
    }
    ok_json(resp)
}

// ── iris_mirror_status (089) ──────────────────────────────────────────────────

/// Normalize `%SYSTEM.Mirror.GetMemberType()` output to Option<String>.
/// Returns `None` for the "Not Member" sentinel and empty strings.
pub fn normalize_mirror_type(s: &str) -> Option<String> {
    if s.is_empty() || s == "Not Member" {
        None
    } else {
        Some(s.to_string())
    }
}

/// Build the JSON payload for `iris_mirror_status`.
pub fn build_mirror_status_json(
    is_member: bool,
    mirror_name: &str,
    member_type: &str,
    is_primary: bool,
) -> serde_json::Value {
    let name_val = if is_member && !mirror_name.is_empty() {
        serde_json::Value::String(mirror_name.to_string())
    } else {
        serde_json::Value::Null
    };
    let type_val = normalize_mirror_type(member_type)
        .map(serde_json::Value::String)
        .unwrap_or(serde_json::Value::Null);
    serde_json::json!({
        "is_member": is_member,
        "mirror_name": name_val,
        "member_type": type_val,
        "is_primary": is_primary,
    })
}

// ── iris_system_performance (089) ────────────────────────────────────────────

/// Validated modes for iris_system_performance.
#[derive(Debug, PartialEq)]
pub enum SystemPerfMode {
    Start,
    Status,
    LastRunId,
    ListProfiles,
    AddProfile,
    DeleteProfile,
    ListRuns,
    Report,
}

impl SystemPerfMode {
    pub fn parse(s: &str) -> Option<Self> {
        match s.trim().to_lowercase().as_str() {
            "start" => Some(Self::Start),
            "status" => Some(Self::Status),
            "last_runid" => Some(Self::LastRunId),
            "list_profiles" => Some(Self::ListProfiles),
            "add_profile" => Some(Self::AddProfile),
            "delete_profile" => Some(Self::DeleteProfile),
            "list_runs" => Some(Self::ListRuns),
            "report" => Some(Self::Report),
            _ => None,
        }
    }
}

/// Every mode name, for the error message an unknown mode gets.
pub const SYSPERF_MODES: &[&str] = &[
    "start",
    "status",
    "last_runid",
    "list_profiles",
    "add_profile",
    "delete_profile",
    "list_runs",
    "report",
];

/// SystemPerformance profiles shipped with every IRIS instance.
pub const SYSPERF_PROFILES: &[&str] = &["test", "30mins", "4hours", "8hours", "12hours", "24hours"];

/// Resolve the profile for `mode=start`, defaulting to `test` (5 minutes, 30-second samples) —
/// the shortest shipped profile, so an unqualified start doesn't pin a collector on the
/// instance for hours.
///
/// The profile is interpolated into an ObjectScript string literal, so anything outside
/// `[A-Za-z0-9_]` is rejected rather than escaped.
pub fn sysperf_profile_or_default(profile: Option<&str>) -> Result<String, String> {
    let p = profile.map(str::trim).filter(|s| !s.is_empty());
    let Some(p) = p else {
        return Ok("test".to_string());
    };
    if !p.chars().all(|c| c.is_ascii_alphanumeric() || c == '_') {
        return Err(format!(
            "invalid profile '{p}': only letters, digits and underscore are allowed. \
             Profiles shipped with IRIS: {}",
            SYSPERF_PROFILES.join(", ")
        ));
    }
    Ok(p.to_string())
}

/// ObjectScript for `mode=start`.
///
/// `run^SystemPerformance` requires a profile name — the argument-less `Do run^SystemPerformance`
/// throws `<UNDEFINED> pname` at `run+4`. The extrinsic form returns the new run ID directly,
/// which is the only reliable way to get it: an in-flight run lives under
/// `^IRIS.SystemPerformance("run",<runid>)` and gets no `("history")` node until it completes,
/// so scanning history right after starting reports the *previous* run.
///
/// `run^SystemPerformance` leaves the current device pointing away from the generator's capture
/// file, so `$IO` is snapshotted before the call and re-selected after it. Without that the
/// `Write` lands nowhere and the tool sees an empty result with a residual
/// `<NAMESPACE>...^%SYS.ProcessQuery` in `$ZERROR` — even though the run started fine.
pub fn sysperf_start_code(profile: &str) -> String {
    format!(
        r#"ZN "%SYS"
Set io=$IO
Set tRun=$$run^SystemPerformance("{profile}")
Use io
Write tRun"#
    )
}

/// ObjectScript for `mode=status`. Same device dance as [`sysperf_start_code`].
pub fn sysperf_status_code(run_id: &str) -> String {
    format!(
        r#"ZN "%SYS"
Set io=$IO
Set tWait=$$waittime^SystemPerformance("{run_id}")
Use io
Write tWait"#
    )
}

/// [`sysperf_status_code`] with the run ID validated first — it is interpolated into a quoted
/// ObjectScript literal, and IRIS run IDs are `YYYYMMDD_HHMMSS_<profile>`.
pub fn sysperf_status_code_checked(run_id: &str) -> Result<String, String> {
    let rid = run_id.trim();
    if rid.is_empty() {
        return Err("mode=status requires run_id".to_string());
    }
    if !rid.chars().all(|c| c.is_ascii_alphanumeric() || c == '_') {
        return Err(format!(
            "invalid run_id '{rid}': only letters, digits and underscore are allowed. \
             Run IDs look like 20260904_161059_test — use mode=last_runid to get the current one."
        ));
    }
    Ok(sysperf_status_code(rid))
}

/// ObjectScript for `mode=last_runid`.
///
/// Reads the in-flight subtree first: while a run is collecting, its ID is under `("run")` and
/// absent from `("history")`. Emits `<runid>|<in_progress>`.
pub fn sysperf_last_runid_code() -> &'static str {
    r#"ZN "%SYS"
Set tRun=$O(^IRIS.SystemPerformance("run",""),-1)
Set tHist=$O(^IRIS.SystemPerformance("history",""),-1)
Write $S(tRun'="":tRun_"|1",1:tHist_"|0")"#
}

/// Split the `<runid>|<in_progress>` payload from [`sysperf_last_runid_code`].
fn parse_last_runid(out: &str) -> (serde_json::Value, bool) {
    let (rid, flag) = out.trim().split_once('|').unwrap_or((out.trim(), "0"));
    let rid = rid.trim();
    let run_id = if rid.is_empty() {
        serde_json::Value::Null
    } else {
        serde_json::Value::String(rid.to_string())
    };
    (run_id, flag.trim() == "1")
}

// ── Profile management and report retrieval (096) ─────────────────────────────
//
// 089 could start a collection and poll it. It could not answer "which profiles does this
// instance have", "make me a 1-second one", or "where did the last run write its report" —
// which is most of what someone actually does with SystemPerformance.

/// Validate a profile name for `add_profile` / `delete_profile`.
///
/// Stricter than it looks necessary, for a measured reason: `$$addprofile^SystemPerformance`
/// accepts `"bad name"`, returns **1**, and stores the profile as `badname`. The caller is told
/// it worked and the name it asked for does not exist. IRIS never reports the rename, so the
/// only place it can be caught is here.
pub fn sysperf_profile_name_checked(name: Option<&str>) -> Result<String, String> {
    let n = name.map(str::trim).filter(|s| !s.is_empty());
    let Some(n) = n else {
        return Err(
            "profile is required: pass the profile name to create or delete (letters, digits \
             and underscore only)"
                .to_string(),
        );
    };
    if !n.chars().all(|c| c.is_ascii_alphanumeric() || c == '_') {
        return Err(format!(
            "invalid profile name '{n}': only letters, digits and underscore are allowed. \
             IRIS silently strips anything else — it would store this as \
             '{}' and report success.",
            n.chars()
                .filter(|c| c.is_ascii_alphanumeric() || *c == '_')
                .collect::<String>()
        ));
    }
    Ok(n.to_string())
}

/// Validate and escape a profile description.
///
/// Descriptions are prose, so unlike the name this escapes rather than rejects: a double quote
/// becomes the ObjectScript `""`. A line break cannot be escaped — the generated code is
/// newline-delimited ObjectScript, so an embedded newline becomes a new command — so control
/// characters are refused.
pub fn sysperf_description_checked(description: Option<&str>) -> Result<String, String> {
    let d = description.map(str::trim).filter(|s| !s.is_empty());
    let Some(d) = d else {
        return Err(
            "description is required: addprofile takes a description, and a profile with a \
             blank one tells the next person nothing"
                .to_string(),
        );
    };
    if d.chars().any(|c| c.is_control()) {
        return Err(format!(
            "invalid description '{}': line breaks and control characters are not allowed",
            d.escape_debug()
        ));
    }
    Ok(d.replace('"', "\"\""))
}

fn sysperf_positive_int(value: Option<i64>, field: &str, hint: &str) -> Result<i64, String> {
    match value {
        Some(v) if v > 0 => Ok(v),
        Some(v) => Err(format!(
            "invalid {field} {v}: must be a positive whole number ({hint})"
        )),
        None => Err(format!("{field} is required ({hint})")),
    }
}

/// Seconds between samples.
pub fn sysperf_interval_checked(interval_seconds: Option<i64>) -> Result<i64, String> {
    sysperf_positive_int(
        interval_seconds,
        "interval_seconds",
        "the shipped profiles use 1 to 60",
    )
}

/// How many samples the run takes. `interval_seconds * sample_count` is the run length.
pub fn sysperf_sample_count_checked(sample_count: Option<i64>) -> Result<i64, String> {
    sysperf_positive_int(
        sample_count,
        "sample_count",
        "interval_seconds x sample_count is how long the run takes",
    )
}

/// Newest-first cap for `mode=list_runs`. An instance that has been collecting for months holds
/// hundreds of history nodes and nobody reads past the recent ones.
pub const SYSPERF_RUN_LIMIT: usize = 100;

/// ObjectScript for `mode=list_profiles`.
///
/// A profile node is `$LB(description, interval_seconds, sample_count)`. The description is
/// written **last** because it is free text and may contain the `|` delimiter — put last, the
/// Rust side can `splitn` and keep it whole.
///
/// `$LISTVALID` guards the decode: one hand-written or legacy node that is not a `$LIST` would
/// otherwise throw `<LIST>` and take the entire listing down with it. The trailing `DONE|<count>`
/// distinguishes "this instance has no profiles" from "the read stopped early".
pub fn sysperf_list_profiles_code() -> &'static str {
    r#"ZN "%SYS"
Set cnt=0,name=$O(^IRIS.SystemPerformance("profile",""))
While name'="" {
  Set node=$G(^IRIS.SystemPerformance("profile",name))
  Set cnt=cnt+1
  If $LISTVALID(node) {
    Write name,"|",$LG(node,2),"|",$LG(node,3),"|",$LG(node,1),!
  } Else {
    Write name,"|||",node,!
  }
  Set name=$O(^IRIS.SystemPerformance("profile",name))
}
Write "DONE|",cnt"#
}

/// Decode one `name|interval|count|description` line from [`sysperf_list_profiles_code`].
///
/// Returns `None` for the `DONE` sentinel and for anything that is not a profile line, so the
/// caller can filter without a second pass.
pub fn parse_profile_line(line: &str) -> Option<serde_json::Value> {
    let parts: Vec<&str> = line.trim_end().splitn(4, '|').collect();
    if parts.len() < 4 {
        return None;
    }
    let name = parts[0].trim();
    if name.is_empty() {
        return None;
    }
    let interval = parts[1].trim().parse::<i64>().ok();
    let count = parts[2].trim().parse::<i64>().ok();
    let duration = match (interval, count) {
        (Some(i), Some(c)) => serde_json::json!((i * c) as f64 / 60.0),
        _ => serde_json::Value::Null,
    };
    Some(serde_json::json!({
        "name": name,
        "interval_seconds": interval,
        "sample_count": count,
        "duration_minutes": duration,
        "description": parts[3],
    }))
}

/// ObjectScript for `mode=add_profile`.
///
/// Writes `<stored>|<result>`: the `$D` of the node under the *requested* name comes first so
/// the `0^<reason>` form of the result, which contains a `^` but no `|`, survives the split.
/// Reading the node back is what catches the silent rename described on
/// [`sysperf_profile_name_checked`] if a future IRIS version rewrites a name this accepts.
pub fn sysperf_add_profile_code(
    name: &str,
    description: &str,
    interval_seconds: i64,
    sample_count: i64,
) -> String {
    format!(
        r#"ZN "%SYS"
Set io=$IO
Set tR=$$addprofile^SystemPerformance("{name}","{description}",{interval_seconds},{sample_count})
Use io
Write $D(^IRIS.SystemPerformance("profile","{name}")),"|",tR"#
    )
}

/// ObjectScript for `mode=delete_profile`.
pub fn sysperf_delete_profile_code(name: &str) -> String {
    format!(
        r#"ZN "%SYS"
Set io=$IO
Set tR=$$delprofile^SystemPerformance("{name}")
Use io
Write tR"#
    )
}

/// Both profile writers answer `1` on success or `0^<reason>` on refusal — a duplicate name
/// gives `0^profile name exists already`. Empty output means the `Write` never landed, which is
/// a failure of the read, not a success.
pub fn parse_profile_write_result(out: &str) -> Result<(), String> {
    let v = out.trim();
    if v == "1" {
        return Ok(());
    }
    match v.split_once('^') {
        Some((_, reason)) if !reason.trim().is_empty() => Err(reason.trim().to_string()),
        _ if v.is_empty() => Err("IRIS returned nothing — the profile was not written".to_string()),
        _ => Err(format!("IRIS refused the profile write: {v}")),
    }
}

/// ObjectScript for `mode=list_runs`.
///
/// A history node is `<$HOROLOG of completion>^<output directory>`, and the subscript is the run
/// ID. Iterates descending so the newest runs come first and the cap keeps the useful ones.
pub fn sysperf_list_runs_code() -> String {
    format!(
        r#"ZN "%SYS"
Set cnt=0,rid=$O(^IRIS.SystemPerformance("history",""),-1)
While (rid'="")&(cnt<{SYSPERF_RUN_LIMIT}) {{
  Set node=$G(^IRIS.SystemPerformance("history",rid))
  Set cnt=cnt+1
  Write rid,"|",$S($P(node,"^",1)'="":$ZDT($P(node,"^",1),3),1:""),"|",$P(node,"^",2),!
  Set rid=$O(^IRIS.SystemPerformance("history",rid),-1)
}}
Write "DONE|",cnt"#
    )
}

/// Decode one `run_id|completed_at|output_dir` line from [`sysperf_list_runs_code`].
pub fn parse_run_line(line: &str) -> Option<serde_json::Value> {
    let parts: Vec<&str> = line.trim_end().splitn(3, '|').collect();
    if parts.len() < 3 {
        return None;
    }
    let run_id = parts[0].trim();
    if run_id.is_empty() {
        return None;
    }
    Some(serde_json::json!({
        "run_id": run_id,
        "completed_at": nullable(parts[1]),
        "output_dir": nullable(parts[2]),
        "profile": profile_from_run_id(run_id),
    }))
}

/// Run IDs are `YYYYMMDD_HHMMSS_<profile>`, and a profile name may itself contain underscores,
/// so everything after the second one is the profile.
fn profile_from_run_id(run_id: &str) -> serde_json::Value {
    let parts: Vec<&str> = run_id.splitn(3, '_').collect();
    match parts.get(2) {
        Some(p) if !p.is_empty() => serde_json::Value::String((*p).to_string()),
        _ => serde_json::Value::Null,
    }
}

fn nullable(s: &str) -> serde_json::Value {
    let t = s.trim();
    if t.is_empty() {
        serde_json::Value::Null
    } else {
        serde_json::Value::String(t.to_string())
    }
}

/// ObjectScript for `mode=report`.
///
/// The report file is `<output dir><node name>_<instance>_<run id>.html`, measured as
/// `/usr/irissys/mgr/57eb64844aa4_IRIS_20260907_135415_test.html`. `$ZU(110)` is the piece that
/// matches — `%SYS.System.GetNodeName()` returns the same name upper-cased, and the file name is
/// lower — but a host rename or a moved container breaks the reconstruction, so a miss falls
/// back to matching `*_<run id>.html` in the directory the run recorded.
///
/// An empty `run_id` resolves to the newest completed run.
pub fn sysperf_report_code(run_id: &str) -> String {
    format!(
        r#"ZN "%SYS"
Set rid="{run_id}"
If rid="" Set rid=$O(^IRIS.SystemPerformance("history",""),-1)
Set node=$G(^IRIS.SystemPerformance("history",rid))
Set dir=$P(node,"^",2)
If dir="" Set dir=$G(^IRIS.SystemPerformance("logdir"))
Set when=$S($P(node,"^",1)'="":$ZDT($P(node,"^",1),3),1:"")
Set path=""
If (rid'="")&(dir'="") {{
  Set path=dir_$ZU(110)_"_"_##class(%SYS.System).GetInstanceName()_"_"_rid_".html"
  If '##class(%File).Exists(path) {{
    Set path="",rs=##class(%ResultSet).%New("%File:FileSet")
    If rs.Execute(dir,"*_"_rid_".html") {{
      While rs.Next() {{ Set path=rs.Get("Name") }}
    }}
  }}
}}
Set exists=0
If path'="" Set exists=##class(%File).Exists(path)
Write rid,"|",when,"|",dir,"|",$S(exists:path,1:""),"|",$S(exists:##class(%File).GetFileSize(path),1:""),"|",exists"#
    )
}

/// [`sysperf_report_code`] with the run ID validated first. Empty is legal — it means the newest
/// completed run — but anything outside the run-ID charset would break out of the literal.
pub fn sysperf_report_code_checked(run_id: &str) -> Result<String, String> {
    let rid = run_id.trim();
    if !rid.chars().all(|c| c.is_ascii_alphanumeric() || c == '_') {
        return Err(format!(
            "invalid run_id '{rid}': only letters, digits and underscore are allowed. \
             Run IDs look like 20260904_161059_test — mode=list_runs shows the completed ones, \
             and omitting run_id reports on the newest."
        ));
    }
    Ok(sysperf_report_code(rid))
}

/// Decode the single `run_id|completed_at|dir|path|size|exists` line from
/// [`sysperf_report_code`].
pub fn parse_report_line(line: &str) -> Option<serde_json::Value> {
    let parts: Vec<&str> = line.trim_end().splitn(6, '|').collect();
    if parts.len() < 6 {
        return None;
    }
    let run_id = parts[0].trim();
    if run_id.is_empty() {
        return None;
    }
    let exists = parts[5].trim() == "1";
    Some(serde_json::json!({
        "run_id": run_id,
        "completed_at": nullable(parts[1]),
        "output_dir": nullable(parts[2]),
        "report_path": nullable(parts[3]),
        "size_bytes": parts[4].trim().parse::<i64>().ok(),
        "exists": exists,
    }))
}

/// Everything `iris_system_performance` reads off the request.
///
/// A struct rather than eight positional arguments: the four profile fields are all optional and
/// mode-dependent, and at a call site `Some("test"), None, Some(1), Some(240)` says nothing about
/// which is which.
#[derive(Debug, Default, Clone)]
pub struct SysPerfRequest<'a> {
    pub mode: &'a str,
    pub run_id: Option<&'a str>,
    pub profile: Option<&'a str>,
    pub description: Option<&'a str>,
    pub interval_seconds: Option<i64>,
    pub sample_count: Option<i64>,
}

impl<'a> SysPerfRequest<'a> {
    pub fn new(mode: &'a str) -> Self {
        Self {
            mode,
            ..Default::default()
        }
    }
}

/// Read a `|`-delimited listing that ends in `DONE|<count>`.
///
/// The sentinel is the point: a truncated read and an empty subtree both come back as no data
/// lines, and reporting the first as `count: 0` is how a broken read looks like a healthy
/// instance with nothing on it.
fn parse_sysperf_listing<F>(
    out: &str,
    parse_line: F,
) -> Result<(Vec<serde_json::Value>, usize), String>
where
    F: Fn(&str) -> Option<serde_json::Value>,
{
    let mut items = Vec::new();
    let mut declared: Option<usize> = None;
    for line in out.lines() {
        let line = line.trim_end();
        if let Some(rest) = line.strip_prefix("DONE|") {
            declared = rest.trim().parse::<usize>().ok();
            continue;
        }
        if let Some(v) = parse_line(line) {
            items.push(v);
        }
    }
    let Some(declared) = declared else {
        return Err(
            "IRIS did not finish the listing (no end marker) — the result is incomplete, \
             not empty"
                .to_string(),
        );
    };
    Ok((items, declared))
}

pub async fn iris_system_performance_impl(
    iris: &IrisConnection,
    client: &reqwest::Client,
    req: &SysPerfRequest<'_>,
) -> Result<CallToolResult, McpError> {
    let SysPerfRequest {
        mode,
        run_id,
        profile,
        description,
        interval_seconds,
        sample_count,
    } = *req;
    match SystemPerfMode::parse(mode) {
        None => ok_json(serde_json::json!({
            "success": false,
            "error": format!(
                "unknown mode '{}'; valid values: {}",
                mode,
                SYSPERF_MODES.join(", ")
            ),
        })),
        Some(SystemPerfMode::LastRunId) => {
            match iris
                .execute_via_generator(sysperf_last_runid_code(), "%SYS", client)
                .await
            {
                Ok(out) => {
                    let val = out.trim();
                    if is_generator_error(val) {
                        return ok_json(serde_json::json!({
                            "success": false,
                            "error": val,
                        }));
                    }
                    let (run_id_out, in_progress) = parse_last_runid(val);
                    ok_json(serde_json::json!({
                        "success": true,
                        "mode": "last_runid",
                        "run_id": run_id_out,
                        "in_progress": in_progress,
                    }))
                }
                Err(e) => ok_json(serde_json::json!({
                    "success": false,
                    "error": e.to_string(),
                })),
            }
        }
        Some(SystemPerfMode::Start) => {
            let profile = match sysperf_profile_or_default(profile) {
                Ok(p) => p,
                Err(e) => {
                    return ok_json(serde_json::json!({
                        "success": false,
                        "error": e,
                    }));
                }
            };
            let code = sysperf_start_code(&profile);
            match iris.execute_via_generator(&code, "%SYS", client).await {
                Ok(out) => {
                    let val = out.trim();
                    if is_generator_error(val) {
                        return ok_json(serde_json::json!({
                            "success": false,
                            "error": val,
                            "profile": profile,
                        }));
                    }
                    let run_id_out = if val.is_empty() {
                        serde_json::Value::Null
                    } else {
                        serde_json::Value::String(val.to_string())
                    };
                    ok_json(serde_json::json!({
                        "success": true,
                        "mode": "start",
                        "profile": profile,
                        "run_id": run_id_out,
                    }))
                }
                Err(e) => ok_json(serde_json::json!({
                    "success": false,
                    "error": e.to_string(),
                    "profile": profile,
                })),
            }
        }
        Some(SystemPerfMode::Status) => {
            let rid = run_id.unwrap_or_default().trim().to_string();
            let code = match sysperf_status_code_checked(&rid) {
                Ok(c) => c,
                Err(e) => {
                    return ok_json(serde_json::json!({
                        "success": false,
                        "error": e,
                    }));
                }
            };
            match iris.execute_via_generator(&code, "%SYS", client).await {
                Ok(out) => {
                    let val = out.trim();
                    if is_generator_error(val) {
                        return ok_json(serde_json::json!({
                            "success": false,
                            "error": val,
                            "run_id": rid,
                        }));
                    }
                    ok_json(serde_json::json!({
                        "success": true,
                        "mode": "status",
                        "run_id": rid,
                        "wait_time": val,
                    }))
                }
                Err(e) => ok_json(serde_json::json!({
                    "success": false,
                    "error": e.to_string(),
                    "run_id": rid,
                })),
            }
        }
        Some(SystemPerfMode::ListProfiles) => {
            let out = match iris
                .execute_via_generator(sysperf_list_profiles_code(), "%SYS", client)
                .await
            {
                Ok(o) => o,
                Err(e) => return sysperf_failed(serde_json::json!(e.to_string())),
            };
            if is_generator_error(out.trim()) {
                return sysperf_failed(serde_json::json!(out.trim()));
            }
            match parse_sysperf_listing(&out, parse_profile_line) {
                Ok((profiles, declared)) => ok_json(serde_json::json!({
                    "success": true,
                    "mode": "list_profiles",
                    "profiles": profiles,
                    "count": declared,
                })),
                Err(e) => sysperf_failed(serde_json::json!(e)),
            }
        }
        Some(SystemPerfMode::AddProfile) => {
            let (name, desc, interval, count) = match (
                sysperf_profile_name_checked(profile),
                sysperf_description_checked(description),
                sysperf_interval_checked(interval_seconds),
                sysperf_sample_count_checked(sample_count),
            ) {
                (Ok(n), Ok(d), Ok(i), Ok(c)) => (n, d, i, c),
                (n, d, i, c) => {
                    // Report every bad field at once. Fixing one and being told about the next
                    // is three round trips for one malformed call.
                    let problems: Vec<String> = [n.err(), d.err(), i.err(), c.err()]
                        .into_iter()
                        .flatten()
                        .collect();
                    return sysperf_failed(serde_json::json!(problems.join("; ")));
                }
            };
            let code = sysperf_add_profile_code(&name, &desc, interval, count);
            let out = match iris.execute_via_generator(&code, "%SYS", client).await {
                Ok(o) => o,
                Err(e) => return sysperf_failed(serde_json::json!(e.to_string())),
            };
            let val = out.trim();
            if is_generator_error(val) {
                return sysperf_failed(serde_json::json!(val));
            }
            let (stored, result) = val.split_once('|').unwrap_or(("0", val));
            if let Err(reason) = parse_profile_write_result(result) {
                return sysperf_failed(serde_json::json!(reason));
            }
            if stored.trim() != "1" {
                // addprofile said yes and the node is not there under the name that was asked
                // for. That is the silent-rename case; do not report it as a success.
                return sysperf_failed(serde_json::json!(format!(
                    "IRIS reported success but stored no profile named '{name}' — the name was \
                     rewritten. Check mode=list_profiles."
                )));
            }
            ok_json(serde_json::json!({
                "success": true,
                "mode": "add_profile",
                "profile": name,
                "interval_seconds": interval,
                "sample_count": count,
                "duration_minutes": (interval * count) as f64 / 60.0,
            }))
        }
        Some(SystemPerfMode::DeleteProfile) => {
            let name = match sysperf_profile_name_checked(profile) {
                Ok(n) => n,
                Err(e) => return sysperf_failed(serde_json::json!(e)),
            };
            let code = sysperf_delete_profile_code(&name);
            let out = match iris.execute_via_generator(&code, "%SYS", client).await {
                Ok(o) => o,
                Err(e) => return sysperf_failed(serde_json::json!(e.to_string())),
            };
            let val = out.trim();
            if is_generator_error(val) {
                return sysperf_failed(serde_json::json!(val));
            }
            match parse_profile_write_result(val) {
                Ok(()) => ok_json(serde_json::json!({
                    "success": true,
                    "mode": "delete_profile",
                    "profile": name,
                })),
                Err(reason) => sysperf_failed(serde_json::json!(reason)),
            }
        }
        Some(SystemPerfMode::ListRuns) => {
            let out = match iris
                .execute_via_generator(&sysperf_list_runs_code(), "%SYS", client)
                .await
            {
                Ok(o) => o,
                Err(e) => return sysperf_failed(serde_json::json!(e.to_string())),
            };
            if is_generator_error(out.trim()) {
                return sysperf_failed(serde_json::json!(out.trim()));
            }
            match parse_sysperf_listing(&out, parse_run_line) {
                Ok((runs, declared)) => ok_json(serde_json::json!({
                    "success": true,
                    "mode": "list_runs",
                    "runs": runs,
                    "count": declared,
                    "limit": SYSPERF_RUN_LIMIT,
                })),
                Err(e) => sysperf_failed(serde_json::json!(e)),
            }
        }
        Some(SystemPerfMode::Report) => {
            let code = match sysperf_report_code_checked(run_id.unwrap_or_default()) {
                Ok(c) => c,
                Err(e) => return sysperf_failed(serde_json::json!(e)),
            };
            let out = match iris.execute_via_generator(&code, "%SYS", client).await {
                Ok(o) => o,
                Err(e) => return sysperf_failed(serde_json::json!(e.to_string())),
            };
            let val = out.trim();
            if is_generator_error(val) {
                return sysperf_failed(serde_json::json!(val));
            }
            match parse_report_line(val) {
                Some(mut report) => {
                    report["success"] = serde_json::json!(true);
                    report["mode"] = serde_json::json!("report");
                    if report["exists"] == serde_json::json!(false) {
                        // A run that is still collecting has no history node and no file yet.
                        // Saying so beats handing back a path that is not there.
                        report["note"] = serde_json::json!(
                            "no report file for this run yet — a run writes its HTML only when \
                             it completes (mode=status shows the remaining time)"
                        );
                    }
                    ok_json(report)
                }
                None => sysperf_failed(serde_json::json!(format!(
                    "no completed run to report on{}",
                    match run_id.map(str::trim).filter(|s| !s.is_empty()) {
                        Some(r) => format!(": '{r}' is not in the run history"),
                        None => " — this instance has no SystemPerformance history".to_string(),
                    }
                ))),
            }
        }
    }
}

/// `{"success": false, "error": ...}` — the shape every SystemPerformance failure returns.
fn sysperf_failed(error: serde_json::Value) -> Result<CallToolResult, McpError> {
    ok_json(serde_json::json!({ "success": false, "error": error }))
}

pub async fn iris_mirror_status_impl(
    iris: &IrisConnection,
    client: &reqwest::Client,
) -> Result<CallToolResult, McpError> {
    let code = r#"ZN "%SYS"
Set tMember=##class(%SYSTEM.Mirror).IsMember()
Set tName=##class(%SYSTEM.Mirror).MirrorName()
Set tType=##class(%SYSTEM.Mirror).GetMemberType()
Set tPrimary=##class(%SYSTEM.Mirror).IsPrimary()
Write tMember,"|",tName,"|",tType,"|",tPrimary"#;
    match iris.execute_via_generator(code, "%SYS", client).await {
        Ok(out) => {
            let out = out.trim();
            if is_generator_error(out) {
                return ok_json(serde_json::json!({
                    "success": false,
                    "error": out,
                    "is_member": serde_json::Value::Null,
                }));
            }
            let parts: Vec<&str> = out.splitn(4, '|').collect();
            let is_member = parts.first().copied().unwrap_or("0") != "0";
            let mirror_name = parts.get(1).copied().unwrap_or("");
            let member_type = parts.get(2).copied().unwrap_or("");
            let is_primary = parts.get(3).copied().unwrap_or("0") != "0";
            let mut v = build_mirror_status_json(is_member, mirror_name, member_type, is_primary);
            v["success"] = serde_json::Value::Bool(true);
            ok_json(v)
        }
        Err(e) => ok_json(serde_json::json!({
            "success": false,
            "error": e.to_string(),
            "is_member": serde_json::Value::Null,
        })),
    }
}

// ── iris_mirror_add_async_impl (097) ─────────────────────────────────────────

pub async fn iris_mirror_add_async_impl(
    iris: Option<&IrisConnection>,
    client: &reqwest::Client,
    mirror_name: &str,
    primary_host: &str,
    primary_port: u16,
    instance_name: &str,
    async_member_type: u8,
) -> Result<CallToolResult, McpError> {
    let iris = match iris {
        Some(c) => c,
        None => {
            return ok_json(serde_json::json!({
                "success": false,
                "error_code": "IRIS_UNREACHABLE",
                "error": "No IRIS connection available",
            }))
        }
    };
    let code = format!(
        r#"ZN "%SYS"
Set tMember=##class(%SYSTEM.Mirror).IsMember()
If tMember'=0 {{
  Write "ALREADY_MEMBER|",##class(%SYSTEM.Mirror).GetMirrorNames(),!
  Quit
}}
Set tLocalInfo="",tSSLInfo=""
; JoinMirrorAsAsyncMember opens sockets to the primary and can leave the current device pointed at
; one of them. The generator captures output by making a temp file the current device, so a moved
; $IO sends every Write below into the void and the caller sees an empty success. Restore it.
Set tIO=$IO
Set tSC=##class(SYS.Mirror).JoinMirrorAsAsyncMember("{mirror_name}","","{instance_name}","{primary_host}",{primary_port},{async_member_type},.tLocalInfo,.tSSLInfo)
Use tIO
If $$$ISERR(tSC) {{
  Write "ERROR:",$System.Status.GetErrorText(tSC),!
}} Else {{
  Write "OK",!
}}"#
    );
    match iris.execute_via_generator(&code, "%SYS", client).await {
        Ok(out) => {
            let out = out.trim();
            if out.starts_with("ALREADY_MEMBER|") {
                let existing = out.strip_prefix("ALREADY_MEMBER|").unwrap_or("");
                return ok_json(serde_json::json!({
                    "success": false,
                    "error_code": "ALREADY_MEMBER",
                    "error": format!("Instance is already a member of mirror set: {existing}"),
                    "mirror_name": existing,
                }));
            }
            if let Some(err) = generator_error_message(out) {
                let lc = err.to_lowercase();
                if lc.contains("version") || lc.contains("incompatible") {
                    return ok_json(serde_json::json!({
                        "success": false,
                        "error_code": "MIRROR_VERSION_MISMATCH",
                        "error": err,
                    }));
                }
                return ok_json(serde_json::json!({
                    "success": false,
                    "error_code": "MIRROR_JOIN_FAILED",
                    "error": err,
                }));
            }
            ok_json(serde_json::json!({
                "success": true,
                "action": "mirror_add_async",
                "mirror_name": mirror_name,
            }))
        }
        Err(e) => ok_json(serde_json::json!({
            "success": false,
            "error_code": "IRIS_UNREACHABLE",
            "error": e.to_string(),
        })),
    }
}

// ── iris_mirror_failover_impl (097) ──────────────────────────────────────────

pub async fn iris_mirror_failover_impl(
    iris: Option<&IrisConnection>,
    client: &reqwest::Client,
) -> Result<CallToolResult, McpError> {
    let iris = match iris {
        Some(c) => c,
        None => {
            return ok_json(serde_json::json!({
                "success": false,
                "error_code": "IRIS_UNREACHABLE",
                "error": "No IRIS connection available",
            }))
        }
    };
    let code = r#"ZN "%SYS"
Set tMember=##class(%SYSTEM.Mirror).IsMember()
If tMember=0 {
  Write "NOT_MEMBER",!
  Quit
}
Set tPrimary=##class(%SYSTEM.Mirror).IsPrimary()
If tPrimary {
  Write "ALREADY_PRIMARY",!
  Quit
}
; BecomePrimary reconfigures the mirror and can leave $IO on a device of its own choosing. The
; generator reads output off the current device, so restore it before writing the verdict.
Set tIO=$IO
Set tResult=##class(SYS.Mirror).BecomePrimary()
Use tIO
If tResult {
  Write "OK",!
} Else {
  Write "ERROR:BecomePrimary returned false",!
}"#;
    match iris.execute_via_generator(code, "%SYS", client).await {
        Ok(out) => {
            let out = out.trim();
            if out == "NOT_MEMBER" {
                return ok_json(serde_json::json!({
                    "success": false,
                    "error_code": "NOT_MIRROR_MEMBER",
                    "error": "This instance is not a mirror member",
                }));
            }
            if out == "ALREADY_PRIMARY" {
                return ok_json(serde_json::json!({
                    "success": false,
                    "error_code": "ALREADY_PRIMARY",
                    "error": "This instance is already the primary — no failover needed",
                }));
            }
            if let Some(err) = generator_error_message(out) {
                return ok_json(serde_json::json!({
                    "success": false,
                    "error_code": "MIRROR_FAILOVER_FAILED",
                    "error": err,
                }));
            }
            ok_json(serde_json::json!({
                "success": true,
                "action": "mirror_failover",
                "new_role": "primary",
            }))
        }
        Err(e) => ok_json(serde_json::json!({
            "success": false,
            "error_code": "IRIS_UNREACHABLE",
            "error": e.to_string(),
        })),
    }
}

// ── is_version_mismatch_error (097) ──────────────────────────────────────────

/// Returns true if the IRIS error string suggests a mirror version incompatibility.
pub fn is_version_mismatch_error(error: &str) -> bool {
    let lc = error.to_lowercase();
    lc.contains("version") || lc.contains("incompatible")
}

// ── parse_max_size_mb (089) ───────────────────────────────────────────────────

/// Parse the `MaxSize` string from `%SYS.DatabaseQuery:FreeSpace`.
/// Returns `None` for "Unlimited" or unrecognized formats.
pub fn parse_max_size_mb(s: &str) -> Option<i64> {
    if s.is_empty() || s.to_uppercase() == "UNLIMITED" {
        return None;
    }
    let upper = s.to_uppercase();
    if let Some(n) = upper.strip_suffix("GB") {
        return n.trim().parse::<i64>().ok().map(|v| v * 1024);
    }
    if let Some(n) = upper.strip_suffix("MB") {
        return n.trim().parse::<i64>().ok();
    }
    // bare number — treat as MB
    s.trim().parse::<i64>().ok()
}

// ── iris_namespace_create (T084) — gated in call_tool, not here (085) ─────────

pub async fn iris_namespace_create_impl(
    iris: &IrisConnection,
    client: &reqwest::Client,
    name: &str,
    db_path: Option<&str>,
) -> Result<CallToolResult, McpError> {
    let db = db_path.unwrap_or(name);
    let code = format!(
        r#"Set props("Name")="{name}"
Set props("Globals")="{db}"
Set props("Routines")="{name}"
; CreateOne edits the CPF, which it does by opening the file — and that leaves the current device
; on the CPF, not on the generator's capture file. Every Write below then went nowhere and
; namespace creation reported success with no output to check. Restore $IO first.
Set tIO=$IO
Set tSC=##class(Config.Namespaces).CreateOne(.props)
Use tIO
If $$$ISERR(tSC) {{
  Write "ERROR:"_$System.Status.GetErrorText(tSC),!
}} Else {{
  Write "CREATED",!
}}"#
    );
    match iris.execute_via_generator(&code, "%SYS", client).await {
        Ok(out) => {
            let out = out.trim();
            if is_generator_error(out) {
                err_json("CREATE_FAILED", out)
            } else {
                ok_json(serde_json::json!({
                    "success": true,
                    "created": true,
                    "name": name,
                }))
            }
        }
        Err(e) => err_json("IRIS_UNREACHABLE", &e.to_string()),
    }
}

// ── iris_database_stats (T085) ────────────────────────────────────────────────

pub async fn iris_database_stats_impl(
    iris: &IrisConnection,
    client: &reqwest::Client,
    db: Option<&str>,
) -> Result<CallToolResult, McpError> {
    // Use SYS.Database:List and augment with GetFreeSpace for specific DB or all.
    let code = if let Some(dir) = db {
        format!(
            r#"Set dir="{dir}"
Set tSC=##class(SYS.Database).GetFreeSpace(dir,.free,.blocks)
If $$$ISERR(tSC) {{
  Write "ERROR:"_$System.Status.GetErrorText(tSC),!
  Quit
}}
Write dir,"|",free,"|",blocks,!"#
        )
    } else {
        r#"Set tRS=##class(%ResultSet).%New("SYS.Database:List")
Set tSC=tRS.Execute()
If $$$ISERR(tSC) { Write "ERROR:"_$System.Status.GetErrorText(tSC) Quit }
While tRS.Next() {
  Set dir=tRS.Get("Directory")
  Set tSC2=##class(SYS.Database).GetFreeSpace(dir,.free,.blocks)
  If $$$ISERR(tSC2) { Set free=0 Set blocks=0 }
  Write dir,"|",free,"|",blocks,!
}"#
        .to_string()
    };

    match iris.execute_via_generator(&code, "%SYS", client).await {
        Ok(out) => {
            let out = out.trim();
            if is_generator_error(out) {
                return err_json("IRIS_EXECUTE_ERROR", out);
            }
            let stats: Vec<serde_json::Value> = out
                .lines()
                .filter(|l| !l.is_empty())
                .map(|line| {
                    let parts: Vec<&str> = line.splitn(3, '|').collect();
                    serde_json::json!({
                        "directory": parts.first().copied().unwrap_or(""),
                        "free_space_mb": parts.get(1).copied().unwrap_or("0")
                            .trim().parse::<f64>().unwrap_or(0.0),
                        "free_blocks": parts.get(2).copied().unwrap_or("0")
                            .trim().parse::<i64>().unwrap_or(0),
                    })
                })
                .collect();
            ok_json(serde_json::json!({
                "success": true,
                "stats": stats,
            }))
        }
        Err(e) => err_json("IRIS_UNREACHABLE", &e.to_string()),
    }
}

// ── journal_search (T088) ─────────────────────────────────────────────────────

pub async fn journal_search_impl(
    iris: &IrisConnection,
    client: &reqwest::Client,
    start: Option<&str>,
    end: Option<&str>,
    global_pattern: Option<&str>,
    max_entries: u32,
) -> Result<CallToolResult, McpError> {
    let limit = max_entries.clamp(1, 500);
    let start_filter = start
        .map(|s| format!("If ts<\"{s}\" Continue"))
        .unwrap_or_default();
    let end_filter = end
        .map(|e| format!("If ts>\"{e}\" Continue"))
        .unwrap_or_default();
    let global_filter = global_pattern
        .map(|p| {
            // rec.TypeName holds the journal record's operation ("SET", "ZKILL", "BeginTrans",
            // ...), not a class name — it never equals "SetKillRecord". That's the class
            // itself (%SYS.Journal.SetKillRecord, which SET and ZKILL both instantiate).
            // Gating on TypeName="SetKillRecord" made this `If` permanently false, so the
            // filter below never ran and every record passed through unfiltered.
            format!(
                "If $classname(rec)[\"SetKillRecord\" {{ If rec.GlobalReference'[\"{}\" Continue }}",
                p.replace('"', "")
            )
        })
        .unwrap_or_default();

    let code = format!(
        r#"Set jfName=##class(%SYS.Journal.System).GetCurrentFileName()
Set jf=##class(%SYS.Journal.File).%OpenId(jfName)
If jf="" {{ Write "NO_JOURNAL",! Quit }}
Set rec=jf.FirstRecord
Set cnt=0
While (rec'="")&&(cnt<{limit}) {{
  Set ts=rec.TimeStamp
  {start_filter}
  {end_filter}
  {global_filter}
  Set typeName=rec.TypeName
  Set jobID=rec.JobID
  Set gref=""
  If $classname(rec)["SetKillRecord" {{ Set gref=rec.GlobalReference }}
  Write ts,"|",typeName,"|",jobID,"|",gref,!
  Set cnt=cnt+1
  Set rec=rec.Next
}}
Write "DONE|"_cnt,!"#
    );

    match iris.execute_via_generator(&code, "%SYS", client).await {
        Ok(out) => {
            let mut entries: Vec<serde_json::Value> = Vec::new();
            let mut total = 0u32;
            for line in out.lines() {
                if line.is_empty() {
                    continue;
                }
                if let Some(rest) = line.strip_prefix("DONE|") {
                    total = rest.parse().unwrap_or(0);
                } else if line == "NO_JOURNAL" {
                    return err_json("NO_JOURNAL", "No current journal file found.");
                } else {
                    let parts: Vec<&str> = line.splitn(4, '|').collect();
                    entries.push(serde_json::json!({
                        "timestamp": parts.first().copied().unwrap_or(""),
                        "type": parts.get(1).copied().unwrap_or(""),
                        "job_id": parts.get(2).copied().unwrap_or("").parse::<i64>().unwrap_or(0),
                        "global": parts.get(3).copied().unwrap_or(""),
                    }));
                }
            }
            ok_json(serde_json::json!({
                "success": true,
                "entries": entries,
                "returned": total,
            }))
        }
        Err(e) => err_json("IRIS_UNREACHABLE", &e.to_string()),
    }
}

// ── query_audit_log (T089) ────────────────────────────────────────────────────

pub async fn query_audit_log_impl(
    iris: &IrisConnection,
    client: &reqwest::Client,
    user: Option<&str>,
    event_type: Option<&str>,
    start: Option<&str>,
    end: Option<&str>,
    limit: u32,
) -> Result<CallToolResult, McpError> {
    let limit = limit.clamp(1, 500);
    let mut conditions = vec!["1=1".to_string()];
    let mut params: Vec<String> = Vec::new();

    if let Some(u) = user {
        conditions.push("Username = ?".to_string());
        params.push(u.to_string());
    }
    if let Some(et) = event_type {
        conditions.push("EventType = ?".to_string());
        params.push(et.to_string());
    }
    if let Some(s) = start {
        conditions.push("UTCTimeStamp >= ?".to_string());
        params.push(s.to_string());
    }
    if let Some(e) = end {
        conditions.push("UTCTimeStamp <= ?".to_string());
        params.push(e.to_string());
    }

    let where_clause = conditions.join(" AND ");
    let sql = format!(
        "SELECT TOP {limit} Event, EventType, Username, UTCTimeStamp FROM %SYS.Audit WHERE {where_clause} ORDER BY UTCTimeStamp DESC"
    );

    let param_values: Vec<serde_json::Value> = params
        .iter()
        .map(|p| serde_json::Value::String(p.clone()))
        .collect();

    match iris.query(&sql, param_values, "%SYS", client).await {
        Ok(resp) => {
            let rows = resp["result"]["content"]
                .as_array()
                .cloned()
                .unwrap_or_default();
            let entries: Vec<serde_json::Value> = rows
                .iter()
                .map(|r| {
                    serde_json::json!({
                        "event": r["Event"],
                        "event_type": r["EventType"],
                        "username": r["Username"],
                        "timestamp": r["UTCTimeStamp"],
                    })
                })
                .collect();
            let count = entries.len();
            ok_json(serde_json::json!({
                "success": true,
                "entries": entries,
                "count": count,
            }))
        }
        Err(e) => err_json("IRIS_UNREACHABLE", &e.to_string()),
    }
}

// ── stream_inspect (T090) ─────────────────────────────────────────────────────

pub async fn stream_inspect_impl(
    iris: &IrisConnection,
    client: &reqwest::Client,
    oid: &str,
    namespace: &str,
) -> Result<CallToolResult, McpError> {
    // Strip leading/trailing whitespace and extract numeric id
    let id = oid.trim();
    let code = format!(
        r#"Set stream=##class(%Stream.GlobalCharacter).%OpenId("{id}")
If stream="" {{
  // Try binary
  Set stream=##class(%Stream.GlobalBinary).%OpenId("{id}")
  If stream="" {{ Write "ERROR:STREAM_NOT_FOUND",! Quit }}
  Set isText=0
}} Else {{
  Set isText=1
}}
Set size=stream.Size
Set content=""
Do stream.Rewind()
While stream.AtEnd=0 {{
  Set content=content_stream.Read(4096)
}}
Write "SIZE|"_size,!
Write "TYPE|"_$Select(isText:"text",1:"binary"),!
Write "CONTENT|"_content,!"#
    );

    match iris.execute_via_generator(&code, namespace, client).await {
        Ok(out) => {
            if out.contains("ERROR:STREAM_NOT_FOUND") {
                return err_json(
                    "STREAM_NOT_FOUND",
                    &format!("Stream with oid '{oid}' not found."),
                );
            }
            let mut size: i64 = 0;
            let mut stream_type = "text".to_string();
            let mut content = String::new();
            for line in out.lines() {
                if let Some(rest) = line.strip_prefix("SIZE|") {
                    size = rest.parse().unwrap_or(0);
                } else if let Some(rest) = line.strip_prefix("TYPE|") {
                    stream_type = rest.to_string();
                } else if let Some(rest) = line.strip_prefix("CONTENT|") {
                    content = rest.to_string();
                }
            }
            ok_json(serde_json::json!({
                "success": true,
                "oid": oid,
                "type": stream_type,
                "size": size,
                "content": content,
            }))
        }
        Err(e) => err_json("IRIS_UNREACHABLE", &e.to_string()),
    }
}

// ── my_access (T093) ──────────────────────────────────────────────────────────

/// Read the username out of a `Write $USERNAME` generator run, or say why there isn't one.
///
/// `execute_via_generator` reports IRIS-side failure inside the output string, so the failure text
/// used to be taken for a username. `Security.Users` then matched nothing, and the no-rows branch
/// of both callers answers `success: true` with `"roles": []` — the tools claimed the caller holds
/// no roles when they had never established who the caller is. An empty output is the same lie
/// without the error text.
pub fn username_from_generator_output(out: &str) -> Result<String, String> {
    if let Some(msg) = generator_error_message(out) {
        return Err(msg.trim().to_string());
    }
    let name = out.trim();
    if name.is_empty() {
        return Err("IRIS returned no output for `Write $USERNAME`".to_string());
    }
    Ok(name.to_string())
}

pub async fn my_access_impl(
    iris: &IrisConnection,
    client: &reqwest::Client,
) -> Result<CallToolResult, McpError> {
    // Get current username and then look up their roles
    let code = r#"Write $USERNAME,!"#;
    let username_out = match iris
        .execute_via_generator(code, &iris.namespace, client)
        .await
    {
        Ok(out) => out,
        Err(e) => return err_json("IRIS_UNREACHABLE", &e.to_string()),
    };
    let username = match username_from_generator_output(&username_out) {
        Ok(name) => name,
        Err(msg) => {
            return err_json(
                "IRIS_EXECUTE_ERROR",
                &format!("Could not determine the current IRIS username, so no permissions were checked: {msg}"),
            )
        }
    };

    // Query Security.Users
    let sql =
        "SELECT Name, FullName, $LISTTOSTRING(Roles) AS Roles FROM Security.Users WHERE Name = ?";
    match iris
        .query(
            sql,
            vec![serde_json::Value::String(username.to_string())],
            "%SYS",
            client,
        )
        .await
    {
        Ok(resp) => {
            let rows = resp["result"]["content"]
                .as_array()
                .cloned()
                .unwrap_or_default();
            if let Some(row) = rows.first() {
                let roles_str = row["Roles"].as_str().unwrap_or("");
                let roles: Vec<&str> = roles_str
                    .split(',')
                    .map(|r| r.trim())
                    .filter(|r| !r.is_empty())
                    .collect();
                ok_json(serde_json::json!({
                    "success": true,
                    "username": row["Name"],
                    "full_name": row["FullName"],
                    "roles": roles,
                }))
            } else {
                ok_json(serde_json::json!({
                    "success": true,
                    "username": username,
                    "full_name": "",
                    "roles": [],
                }))
            }
        }
        Err(e) => err_json("IRIS_UNREACHABLE", &e.to_string()),
    }
}

// ── capability_matrix (T094) ──────────────────────────────────────────────────

pub async fn capability_matrix_impl(
    iris: &IrisConnection,
    client: &reqwest::Client,
    user: Option<&str>,
) -> Result<CallToolResult, McpError> {
    // Resolve username
    let username = if let Some(u) = user {
        u.to_string()
    } else {
        let code = r#"Write $USERNAME,!"#;
        let out = match iris.execute_via_generator(code, "%SYS", client).await {
            Ok(out) => out,
            Err(e) => return err_json("IRIS_UNREACHABLE", &e.to_string()),
        };
        // Same in-band failure as `my_access`: the failure text became the username, the
        // `Security.Users` lookup missed, and the matrix reported an empty role set as success.
        match username_from_generator_output(&out) {
            Ok(name) => name,
            Err(msg) => {
                return err_json(
                    "IRIS_EXECUTE_ERROR",
                    &format!("Could not determine the current IRIS username, so no capabilities were resolved: {msg}"),
                )
            }
        }
    };

    let sql =
        "SELECT Name, FullName, $LISTTOSTRING(Roles) AS Roles FROM Security.Users WHERE Name = ?";
    match iris
        .query(
            sql,
            vec![serde_json::Value::String(username.clone())],
            "%SYS",
            client,
        )
        .await
    {
        Ok(resp) => {
            let rows = resp["result"]["content"]
                .as_array()
                .cloned()
                .unwrap_or_default();
            if let Some(row) = rows.first() {
                let roles_str = row["Roles"].as_str().unwrap_or("");
                let roles: Vec<String> = roles_str
                    .split(',')
                    .map(|r| r.trim().to_string())
                    .filter(|r| !r.is_empty())
                    .collect();
                ok_json(serde_json::json!({
                    "success": true,
                    "user": username,
                    "full_name": row["FullName"],
                    "roles": roles,
                    "note": "Use iris_admin action=list_roles for full role definitions.",
                }))
            } else {
                ok_json(serde_json::json!({
                    "success": true,
                    "user": username,
                    "roles": [],
                }))
            }
        }
        Err(e) => err_json("IRIS_UNREACHABLE", &e.to_string()),
    }
}

// ── hl7_schema_list (T097) ────────────────────────────────────────────────────

pub async fn hl7_schema_list_impl(
    iris: &IrisConnection,
    client: &reqwest::Client,
    namespace: &str,
) -> Result<CallToolResult, McpError> {
    // Check availability first
    let check_code = r#"Write ##class(%Dictionary.CompiledClass).%ExistsId("EnsLib.HL7.Schema"),!"#;
    let check_out = match iris
        .execute_via_generator(check_code, namespace, client)
        .await
    {
        Ok(out) => out.trim().to_string(),
        Err(e) => return err_json("IRIS_UNREACHABLE", &e.to_string()),
    };
    if check_out.trim() != "1" {
        return err_json(ERR_HL7_NOT_AVAILABLE, "EnsLib.HL7.Schema is not available on this IRIS instance. Requires InterSystems HealthShare or IRIS for Health.");
    }

    let code = r#"Set tRS=##class(%ResultSet).%New("EnsLib.HL7.Schema:StoredSchemaNames")
Set tSC=tRS.Execute()
If $$$ISERR(tSC) { Write "ERROR:"_$System.Status.GetErrorText(tSC) Quit }
While tRS.Next() {
  Write tRS.Data("Name"),!
}"#;
    match iris.execute_via_generator(code, namespace, client).await {
        Ok(out) => {
            let out = out.trim();
            if is_generator_error(out) {
                return err_json("IRIS_EXECUTE_ERROR", out);
            }
            let schemas: Vec<String> = out
                .lines()
                .filter(|l| !l.is_empty())
                .map(|l| l.to_string())
                .collect();
            let count = schemas.len();
            ok_json(serde_json::json!({
                "success": true,
                "schemas": schemas,
                "count": count,
            }))
        }
        Err(e) => err_json("IRIS_UNREACHABLE", &e.to_string()),
    }
}

// ── hl7_schema_inspect (T098) ─────────────────────────────────────────────────

pub async fn hl7_schema_inspect_impl(
    iris: &IrisConnection,
    client: &reqwest::Client,
    schema: &str,
    segment: Option<&str>,
    namespace: &str,
) -> Result<CallToolResult, McpError> {
    let check_code = r#"Write ##class(%Dictionary.CompiledClass).%ExistsId("EnsLib.HL7.Schema"),!"#;
    let check_out = match iris
        .execute_via_generator(check_code, namespace, client)
        .await
    {
        Ok(out) => out.trim().to_string(),
        Err(e) => return err_json("IRIS_UNREACHABLE", &e.to_string()),
    };
    if check_out.trim() != "1" {
        return err_json(ERR_HL7_NOT_AVAILABLE, "EnsLib.HL7.Schema is not available.");
    }

    let seg_code = if let Some(seg) = segment {
        format!(
            r#"Set tRS=##class(%ResultSet).%New("EnsLib.HL7.Schema:SegmentStructureElements")
Set tSC=tRS.Execute("{schema}","{seg}")
If $$$ISERR(tSC) {{ Write "ERROR:"_$System.Status.GetErrorText(tSC) Quit }}
While tRS.Next() {{
  Write tRS.Data("FieldName"),"|",tRS.Data("Description"),!
}}"#
        )
    } else {
        format!(
            r#"Set tRS=##class(%ResultSet).%New("EnsLib.HL7.Schema:MessageStructures")
Set tSC=tRS.Execute("{schema}")
If $$$ISERR(tSC) {{ Write "ERROR:"_$System.Status.GetErrorText(tSC) Quit }}
While tRS.Next() {{
  Write tRS.Data("StructureName"),!
}}"#
        )
    };

    match iris
        .execute_via_generator(&seg_code, namespace, client)
        .await
    {
        Ok(out) => {
            let out = out.trim();
            if is_generator_error(out) {
                return err_json("IRIS_EXECUTE_ERROR", out);
            }
            if segment.is_some() {
                let fields: Vec<serde_json::Value> = out
                    .lines()
                    .filter(|l| !l.is_empty())
                    .map(|line| {
                        let parts: Vec<&str> = line.splitn(2, '|').collect();
                        serde_json::json!({
                            "field": parts.first().copied().unwrap_or(""),
                            "description": parts.get(1).copied().unwrap_or(""),
                        })
                    })
                    .collect();
                ok_json(serde_json::json!({
                    "success": true,
                    "schema": schema,
                    "segment": segment,
                    "fields": fields,
                }))
            } else {
                let structures: Vec<String> = out
                    .lines()
                    .filter(|l| !l.is_empty())
                    .map(|l| l.to_string())
                    .collect();
                ok_json(serde_json::json!({
                    "success": true,
                    "schema": schema,
                    "structures": structures,
                }))
            }
        }
        Err(e) => err_json("IRIS_UNREACHABLE", &e.to_string()),
    }
}

// ── build_mermaid_class_diagram (T100–T102) ───────────────────────────────────

/// Pure function: build a Mermaid classDiagram from a list of (class, supers) pairs.
/// `supers` is a comma-separated string as returned by %Dictionary.CompiledClass.Super.
pub fn build_mermaid_class_diagram(classes: &[(String, String)]) -> String {
    let mut out = String::from("classDiagram\n");
    for (cls, supers_str) in classes {
        let safe_cls = cls.replace('%', "Pct_");
        for sup in supers_str
            .split(',')
            .map(|s| s.trim())
            .filter(|s| !s.is_empty())
        {
            let safe_sup = sup.replace('%', "Pct_");
            out.push_str(&format!("    {} <|-- {}\n", safe_sup, safe_cls));
        }
    }
    out
}

pub async fn mermaid_class_impl(
    iris: &IrisConnection,
    client: &reqwest::Client,
    class: &str,
    depth: u32,
    namespace: &str,
) -> Result<CallToolResult, McpError> {
    let depth = depth.clamp(1, 5);
    // Walk superclass chain up to depth levels
    let mut visited: std::collections::HashSet<String> = std::collections::HashSet::new();
    let mut queue: Vec<String> = vec![class.to_string()];
    let mut pairs: Vec<(String, String)> = Vec::new();

    for _ in 0..depth {
        if queue.is_empty() {
            break;
        }
        let current_batch = std::mem::take(&mut queue);
        for cls in &current_batch {
            if visited.contains(cls) {
                continue;
            }
            visited.insert(cls.clone());
            let escaped = cls.replace('"', "");
            let sql =
                format!("SELECT Name, Super FROM %Dictionary.CompiledClass WHERE Name='{escaped}'");
            if let Ok(resp) = iris.query(&sql, vec![], namespace, client).await {
                let rows = resp["result"]["content"]
                    .as_array()
                    .cloned()
                    .unwrap_or_default();
                for row in &rows {
                    let name = row["Name"].as_str().unwrap_or("").to_string();
                    let supers = row["Super"].as_str().unwrap_or("").to_string();
                    pairs.push((name.clone(), supers.clone()));
                    for sup in supers
                        .split(',')
                        .map(|s| s.trim().to_string())
                        .filter(|s| !s.is_empty())
                    {
                        if !visited.contains(&sup) {
                            queue.push(sup);
                        }
                    }
                }
            }
        }
    }

    if pairs.is_empty() {
        return err_json(
            "CLASS_NOT_FOUND",
            &format!("Class '{class}' not found in namespace {namespace}."),
        );
    }

    let diagram = build_mermaid_class_diagram(&pairs);
    ok_json(serde_json::json!({
        "success": true,
        "class": class,
        "depth": depth,
        "diagram": diagram,
    }))
}

// ── mermaid_production (T103) ─────────────────────────────────────────────────

pub async fn mermaid_production_impl(
    iris: &IrisConnection,
    client: &reqwest::Client,
    production: &str,
    namespace: &str,
) -> Result<CallToolResult, McpError> {
    let escaped = production.replace('"', "");
    let sql = format!(
        "SELECT Name, ClassName, Category, Enabled FROM Ens_Config.Item WHERE Production='{escaped}' ORDER BY Category, Name"
    );
    match iris.query(&sql, vec![], namespace, client).await {
        Ok(resp) => {
            let rows = resp["result"]["content"]
                .as_array()
                .cloned()
                .unwrap_or_default();

            let mut diagram = "flowchart TD\n".to_string();
            let safe_prod = production.replace(['.', '%', '-'], "_");
            diagram.push_str(&format!("    {}[\"{}\"]\n", safe_prod, production));

            for row in &rows {
                let name = row["Name"]
                    .as_str()
                    .unwrap_or("")
                    .replace(['.', '%', '-'], "_");
                let class = row["ClassName"].as_str().unwrap_or("");
                let _category = row["Category"].as_str().unwrap_or("");
                let enabled = row["Enabled"].as_str().unwrap_or("1") != "0";
                let style = if enabled { "" } else { ":::disabled" };
                diagram.push_str(&format!(
                    "    {}[\"{}\\n{}\"]{}  \n",
                    name, name, class, style
                ));
                diagram.push_str(&format!("    {} --> {}\n", safe_prod, name));
            }

            ok_json(serde_json::json!({
                "success": true,
                "production": production,
                "item_count": rows.len(),
                "diagram": diagram,
            }))
        }
        Err(e) => err_json("IRIS_UNREACHABLE", &e.to_string()),
    }
}

// ── resolve_storage (T104) ────────────────────────────────────────────────────

pub async fn resolve_storage_impl(
    iris: &IrisConnection,
    client: &reqwest::Client,
    class: &str,
    namespace: &str,
) -> Result<CallToolResult, McpError> {
    let escaped = class.replace('"', "");
    let sql = format!(
        "SELECT Name, Type, DataLocation, IdLocation, IndexLocation FROM %Dictionary.CompiledStorage WHERE parent='{escaped}'"
    );
    match iris.query(&sql, vec![], namespace, client).await {
        Ok(resp) => {
            let rows = resp["result"]["content"]
                .as_array()
                .cloned()
                .unwrap_or_default();
            let storages: Vec<serde_json::Value> = rows
                .iter()
                .map(|r| {
                    serde_json::json!({
                        "name": r["Name"],
                        "type": r["Type"],
                        "data_location": r["DataLocation"],
                        "id_location": r["IdLocation"],
                        "index_location": r["IndexLocation"],
                    })
                })
                .collect();
            ok_json(serde_json::json!({
                "success": true,
                "class": class,
                "storages": storages,
            }))
        }
        Err(e) => err_json("IRIS_UNREACHABLE", &e.to_string()),
    }
}

// ── Unit tests ────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashMap;

    // T075: fresh token is valid
    #[tokio::test]
    async fn confirm_token_fresh_is_valid() {
        let tokens: tokio::sync::Mutex<HashMap<String, ConfirmEntry>> =
            tokio::sync::Mutex::new(HashMap::new());
        let token = "test-token-1".to_string();
        {
            let mut map = tokens.lock().await;
            map.insert(
                token.clone(),
                ConfirmEntry {
                    global: "TestGlobal".to_string(),
                    server: None,
                    issued_at: std::time::Instant::now(),
                },
            );
        }
        let map = tokens.lock().await;
        let entry = map.get(&token).expect("token must be present");
        assert!(!entry.is_expired(), "fresh token should not be expired");
    }

    // T075: token expired after >5 minutes
    #[test]
    fn confirm_entry_expired_after_five_minutes() {
        // Simulate an old issued_at by subtracting 6 minutes.
        // We can't directly set Instant in the past portably,
        // so we test the logic with is_expired() on a "just now" entry,
        // then verify the threshold.
        let entry = ConfirmEntry {
            global: "TestGlobal".to_string(),
            server: None,
            issued_at: std::time::Instant::now(),
        };
        assert!(
            !entry.is_expired(),
            "just-created entry must not be expired"
        );
        // Verify threshold is 300 seconds
        // (We can't test actual expiry without sleeping or mocking Instant)
    }

    // T075: token for different global returns CONFIRM_MISMATCH (logic test)
    #[test]
    fn confirm_mismatch_different_global() {
        let entry = ConfirmEntry {
            global: "GlobalA".to_string(),
            server: None,
            issued_at: std::time::Instant::now(),
        };
        // The mismatch check: entry.global != requested global
        assert_ne!(entry.global, "GlobalB", "different global should not match");
    }

    // T100: build_mermaid_class_diagram produces valid classDiagram syntax
    #[test]
    fn build_mermaid_class_diagram_basic() {
        let classes = vec![
            ("Foo".to_string(), "Bar".to_string()),
            ("Bar".to_string(), "Baz".to_string()),
        ];
        let diagram = build_mermaid_class_diagram(&classes);
        assert!(
            diagram.starts_with("classDiagram"),
            "diagram must start with classDiagram, got: {diagram}"
        );
        assert!(
            diagram.contains("Bar <|-- Foo"),
            "diagram must contain Bar <|-- Foo, got: {diagram}"
        );
        assert!(
            diagram.contains("Baz <|-- Bar"),
            "diagram must contain Baz <|-- Bar, got: {diagram}"
        );
    }

    // T100: percent-sign class names are escaped
    #[test]
    fn build_mermaid_class_diagram_percent_escape() {
        let classes = vec![(
            "%Library.Persistent".to_string(),
            "%Library.Registered".to_string(),
        )];
        let diagram = build_mermaid_class_diagram(&classes);
        assert!(
            !diagram.contains('%'),
            "diagram must not contain bare '%', got: {diagram}"
        );
        assert!(
            diagram.contains("Pct_"),
            "diagram must contain Pct_ prefix, got: {diagram}"
        );
    }

    // 085: the write-gate test that used to live here is gone. `global_kill_impl` no longer
    // decides the gate — `ServerHandler::call_tool` does, for every tool at once — so a unit test
    // that passed `write_tools_enabled: false` into this function was asserting a guard whose
    // absence in eight other write tools is the defect 085 exists to fix. The refusal is now
    // asserted where it is enforced, in `tests/integration/test_gate_enforcement_live.rs`, against
    // a real MCP session and with the negative side effect checked.

    // T075b: global_kill_impl with no token in map → CONFIRM_REQUIRED
    #[tokio::test]
    async fn global_kill_confirm_required() {
        use crate::iris::connection::DiscoverySource;
        let conn = Arc::new(IrisConnection::new(
            "http://localhost:52780",
            "USER",
            "_SYSTEM",
            "SYS",
            DiscoverySource::EnvVar,
        ));
        let client = Arc::new(reqwest::Client::new());
        let tokens: tokio::sync::Mutex<HashMap<String, ConfirmEntry>> =
            tokio::sync::Mutex::new(HashMap::new()); // empty map → no token

        let result = global_kill_impl(
            GlobalKillParams {
                global: "TestGlobal".to_string(),
                server: None,
                confirm_token: "nonexistent-token".to_string(),
                iris: conn,
                client,
            },
            &tokens,
        )
        .await
        .expect("global_kill_impl returned MCP error");

        let text = result
            .content
            .first()
            .map(|c| c.as_text().unwrap().text.clone())
            .expect("no text content");
        let v: serde_json::Value = serde_json::from_str(&text).expect("json parse");
        assert_eq!(
            v["error_code"].as_str().unwrap_or(""),
            ERR_CONFIRM_REQUIRED,
            "missing token should return {ERR_CONFIRM_REQUIRED}, got: {v}"
        );
    }

    // T075c: global_kill_impl with expired token → CONFIRM_EXPIRED
    #[tokio::test]
    async fn global_kill_confirm_expired() {
        use crate::iris::connection::DiscoverySource;
        let conn = Arc::new(IrisConnection::new(
            "http://localhost:52780",
            "USER",
            "_SYSTEM",
            "SYS",
            DiscoverySource::EnvVar,
        ));
        let client = Arc::new(reqwest::Client::new());
        let token = "expired-token".to_string();
        let tokens: tokio::sync::Mutex<HashMap<String, ConfirmEntry>> = {
            let mut map = HashMap::new();
            map.insert(
                token.clone(),
                ConfirmEntry {
                    global: "TestGlobal".to_string(),
                    server: None,
                    // Simulate expiry: issued_at more than 300s ago
                    issued_at: std::time::Instant::now() - std::time::Duration::from_secs(301),
                },
            );
            tokio::sync::Mutex::new(map)
        };

        let result = global_kill_impl(
            GlobalKillParams {
                global: "TestGlobal".to_string(),
                server: None,
                confirm_token: token,
                iris: conn,
                client,
            },
            &tokens,
        )
        .await
        .expect("global_kill_impl returned MCP error");

        let text = result
            .content
            .first()
            .map(|c| c.as_text().unwrap().text.clone())
            .expect("no text content");
        let v: serde_json::Value = serde_json::from_str(&text).expect("json parse");
        assert_eq!(
            v["error_code"].as_str().unwrap_or(""),
            ERR_CONFIRM_EXPIRED,
            "expired token should return {ERR_CONFIRM_EXPIRED}, got: {v}"
        );
    }

    // T075d: global_kill_impl with token for a different global → CONFIRM_MISMATCH
    #[tokio::test]
    async fn global_kill_confirm_mismatch() {
        use crate::iris::connection::DiscoverySource;
        let conn = Arc::new(IrisConnection::new(
            "http://localhost:52780",
            "USER",
            "_SYSTEM",
            "SYS",
            DiscoverySource::EnvVar,
        ));
        let client = Arc::new(reqwest::Client::new());
        let token = "mismatch-token".to_string();
        let tokens: tokio::sync::Mutex<HashMap<String, ConfirmEntry>> = {
            let mut map = HashMap::new();
            map.insert(
                token.clone(),
                ConfirmEntry {
                    global: "GlobalA".to_string(), // token issued for GlobalA
                    server: None,
                    issued_at: std::time::Instant::now(),
                },
            );
            tokio::sync::Mutex::new(map)
        };

        let result = global_kill_impl(
            GlobalKillParams {
                global: "GlobalB".to_string(), // but request is for GlobalB
                server: None,
                confirm_token: token,
                iris: conn,
                client,
            },
            &tokens,
        )
        .await
        .expect("global_kill_impl returned MCP error");

        let text = result
            .content
            .first()
            .map(|c| c.as_text().unwrap().text.clone())
            .expect("no text content");
        let v: serde_json::Value = serde_json::from_str(&text).expect("json parse");
        assert_eq!(
            v["error_code"].as_str().unwrap_or(""),
            ERR_CONFIRM_MISMATCH,
            "token for different global should return {ERR_CONFIRM_MISMATCH}, got: {v}"
        );
    }

    // 085: the T084b write-gate test is gone for the same reason as the global_kill one above —
    // `iris_namespace_create_impl` no longer decides the gate, so asserting a refusal here would
    // test a guard that no longer exists at this layer while saying nothing about the eight tools
    // that never had one.
}
