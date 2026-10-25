//! The stored-source check behind `iris_compile` by name (spec 141, FR-015).
//!
//! A document already on the server was not necessarily written through `iris_doc`: another tool,
//! source control, a routine written to `^rMAC`, an include stored by other means, or a class
//! stored before the gate existed. IRIS runs some constructs in it while it compiles, in the
//! process that serves the compile request. So before `iris_compile` asks IRIS to compile a
//! target, this module reads the stored source of the target and of every `.inc` it includes,
//! recursively, and runs the compile-time scan on each.
//!
//! The check fails closed (constitution XIII). A document that does not exist is left to the
//! compiler to report; a document that exists and cannot be read, for any reason, is refused. So
//! is a wildcard whose members cannot be listed.
//!
//! The walk ([`check_documents`]) takes the reader as a function, so the HTTP reader (Atelier) and
//! the exec reader (`docker exec` into `iris session`, for `docker_only` and NoPWS builds) share
//! one set of rules.

use std::collections::{HashSet, VecDeque};
use std::future::Future;

use regex::Regex;

use crate::iris::connection::IrisConnection;
use crate::policy::code_edit_gate::{check_compile_time_code_mode, included_documents};

const ERR_UNREADABLE: &str = "STORED_SOURCE_UNREADABLE";

/// What reading one stored document gave.
#[derive(Debug)]
pub enum Stored {
    Found(String),
    /// The server says there is no such document. The compiler reports that better than we can.
    Missing,
    /// The document may exist and its source could not be read.
    Unreadable(String),
    /// The channel to IRIS failed (`docker exec`, `ssh`). Carries the error text the caller maps
    /// to its own legible error.
    Transport(String),
}

/// Where the walk starts.
pub enum Seed {
    /// A document to read from the server.
    Stored(String),
    /// A document whose text the caller already holds (a local file about to be uploaded): scanned
    /// as given, and its includes read from the server.
    Content { doc: String, content: String },
}

#[derive(Debug)]
pub enum Outcome {
    Clear,
    /// A refusal body for `err_result`.
    Refused(serde_json::Value),
    Transport(String),
}

/// A comma-separated target list as `$SYSTEM.OBJ.Compile` reads it.
pub fn split_targets(spec: &str) -> Vec<String> {
    spec.split(',')
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(str::to_string)
        .collect()
}

/// A document name for a target: a name with no known extension is a class.
pub fn doc_name_for(target: &str) -> String {
    let lower = target.to_ascii_lowercase();
    if [".cls", ".mac", ".inc", ".int"]
        .iter()
        .any(|ext| lower.ends_with(ext))
    {
        target.to_string()
    } else {
        format!("{target}.cls")
    }
}

/// A target list as `$SYSTEM.OBJ.CompileList` reads it: every part a document name with its
/// extension, which is how the stored-source check names the same documents.
pub fn compile_list_arg(spec: &str) -> String {
    split_targets(spec)
        .iter()
        .map(|t| doc_name_for(t))
        .collect::<Vec<_>>()
        .join(",")
}

/// `*` matches any run of characters, anything else matches itself, case does not matter. Escaped
/// before it is compiled: a pattern that was not a valid regex used to become `.*`, which listed
/// every class in the namespace.
pub fn wildcard_regex(pattern: &str) -> Regex {
    let escaped = regex::escape(pattern).replace("\\*", ".*");
    Regex::new(&format!("(?i)^{escaped}$")).expect("an escaped pattern is a valid regex")
}

fn unreadable(doc: &str, included_by: Option<&str>, reason: &str) -> serde_json::Value {
    let mut v = serde_json::json!({
        "success": false,
        "error_code": ERR_UNREADABLE,
        "document": doc,
        "reason": reason,
        "message": format!(
            "Could not read the stored source of '{doc}' ({reason}), so it cannot be checked for \
             code that runs while it compiles. Nothing was compiled."
        ),
        "remediation":
            "Fix the read (permissions on the document or its database, or the connection) and \
             compile again. A document that does not exist is not refused: the compiler reports it.",
    });
    if let Some(by) = included_by {
        v["included_by"] = serde_json::Value::String(by.to_string());
    }
    v
}

/// Refusal for a wildcard whose members could not be listed.
pub fn unreadable_listing(pattern: &str, reason: &str) -> serde_json::Value {
    unreadable(
        pattern,
        None,
        &format!("the documents matching it could not be listed: {reason}"),
    )
}

/// Read and scan each seed and, recursively, every `.inc` it includes. A document is read at most
/// once, however often it is named and however the includes cycle.
pub async fn check_documents<R, Fut>(mut read: R, seeds: Vec<Seed>) -> Outcome
where
    R: FnMut(String) -> Fut,
    Fut: Future<Output = Stored>,
{
    struct Item {
        doc: String,
        included_by: Option<String>,
        content: Option<String>,
    }
    let mut seen: HashSet<String> = HashSet::new();
    let mut queue: VecDeque<Item> = VecDeque::new();
    for seed in seeds {
        let (doc, content) = match seed {
            Seed::Stored(doc) => (doc, None),
            Seed::Content { doc, content } => (doc, Some(content)),
        };
        if seen.insert(doc.to_ascii_lowercase()) {
            queue.push_back(Item {
                doc,
                included_by: None,
                content,
            });
        }
    }
    while let Some(item) = queue.pop_front() {
        let content = match item.content {
            Some(c) => c,
            None => match read(item.doc.clone()).await {
                Stored::Found(c) => c,
                Stored::Missing => continue,
                Stored::Unreadable(reason) => {
                    return Outcome::Refused(unreadable(
                        &item.doc,
                        item.included_by.as_deref(),
                        &reason,
                    ))
                }
                Stored::Transport(msg) => return Outcome::Transport(msg),
            },
        };
        if let Some(mut refusal) = check_compile_time_code_mode(&content, &item.doc) {
            if let Some(by) = &item.included_by {
                refusal["included_by"] = serde_json::Value::String(by.clone());
            }
            return Outcome::Refused(refusal);
        }
        for include in included_documents(&content, &item.doc) {
            // `%` names are the system's. Their `#define`s hold `##expression` by design (26 of the
            // 211 `%` includes in IRISLIB do), so scanning them would refuse every class that
            // includes `%sySecurity` or anything that reaches it.
            if include.starts_with('%') {
                continue;
            }
            if seen.insert(include.to_ascii_lowercase()) {
                queue.push_back(Item {
                    doc: include,
                    included_by: Some(item.doc.clone()),
                    content: None,
                });
            }
        }
    }
    Outcome::Clear
}

// ── Atelier (HTTP) ───────────────────────────────────────────────────────

/// Read one document over Atelier. 404 is `Missing`; every other failure is `Unreadable`.
pub async fn read_http(
    iris: &IrisConnection,
    client: &reqwest::Client,
    namespace: &str,
    doc: &str,
) -> Stored {
    let url = iris.versioned_ns_url(namespace, &format!("/doc/{}", urlencoding::encode(doc)));
    let resp = match client
        .get(&url)
        .basic_auth(&iris.username, Some(&iris.password))
        .send()
        .await
    {
        Ok(r) => r,
        Err(e) => return Stored::Unreadable(format!("the request failed: {e}")),
    };
    let status = resp.status();
    if status == reqwest::StatusCode::NOT_FOUND {
        return Stored::Missing;
    }
    if !status.is_success() {
        return Stored::Unreadable(format!("HTTP {status}"));
    }
    let body: serde_json::Value =
        match crate::iris::http::capped_json::<serde_json::Value>(resp).await {
            Ok(b) => b,
            Err(e) => return Stored::Unreadable(format!("the reply was not JSON: {e}")),
        };
    if let Some(first) = body["status"]["errors"].as_array().and_then(|a| a.first()) {
        let text = first["error"]
            .as_str()
            .unwrap_or("Atelier reported an error");
        return Stored::Unreadable(text.to_string());
    }
    if !body["result"]["content"].is_array() {
        return Stored::Unreadable("the reply carried no source".to_string());
    }
    Stored::Found(crate::tools::doc::doc_content_to_string(&body))
}

/// The class documents `pattern` names. A pattern with no `*` names itself.
pub async fn expand_http(
    iris: &IrisConnection,
    client: &reqwest::Client,
    namespace: &str,
    pattern: &str,
) -> Result<Vec<String>, String> {
    if !pattern.contains('*') {
        return Ok(vec![pattern.to_string()]);
    }
    let url = iris.versioned_ns_url(namespace, "/docnames/CLS");
    let resp = client
        .get(&url)
        .basic_auth(&iris.username, Some(&iris.password))
        .send()
        .await
        .map_err(|e| format!("the request failed: {e}"))?;
    if !resp.status().is_success() {
        return Err(format!("HTTP {}", resp.status()));
    }
    let body: serde_json::Value = crate::iris::http::capped_json::<serde_json::Value>(resp)
        .await
        .map_err(|e| format!("the reply was not JSON: {e}"))?;
    let names = body["result"]["content"]
        .as_array()
        .ok_or_else(|| "the reply carried no document list".to_string())?;
    let re = wildcard_regex(pattern);
    // /docnames/ returns an array of ({name, cat, ts, ...}), not strings.
    Ok(names
        .iter()
        .filter_map(|d| d["name"].as_str())
        .filter(|n| re.is_match(n))
        .map(str::to_string)
        .collect())
}

/// [`check_documents`] over Atelier. A refusal body, or `None` when the seeds are clear.
pub async fn check_http(
    iris: &IrisConnection,
    client: &reqwest::Client,
    namespace: &str,
    seeds: Vec<Seed>,
) -> Option<serde_json::Value> {
    match check_documents(
        |doc| async move { read_http(iris, client, namespace, &doc).await },
        seeds,
    )
    .await
    {
        Outcome::Clear => None,
        Outcome::Refused(v) => Some(v),
        // The HTTP reader reports its failures as `Unreadable`; this arm keeps the match total.
        Outcome::Transport(msg) => Some(unreadable("the target", None, &msg)),
    }
}

// ── exec (`docker exec` / `ssh` into `iris session`) ─────────────────────

// Markers are written as two string pieces so the marker text is not in the code that prints it.
pub(crate) const MARK: &str = "~IAD\"_\"";

pub(crate) async fn exec(
    iris: &IrisConnection,
    ssh_host: Option<&str>,
    namespace: &str,
    code: &str,
) -> Result<String, String> {
    let result = match ssh_host {
        Some(host) => iris.execute_ssh(code, namespace, host).await,
        None => iris.execute(code, namespace).await,
    };
    result.map_err(|e| e.to_string())
}

/// ObjectScript that prints the stored source of `doc` between `~IAD-BEGIN~` and `~IAD-END~`,
/// `~IAD-MISSING~` when there is no such document, or `~IAD-ERROR~ <text>`.
pub fn read_code(doc: &str) -> String {
    let doc_expr = crate::objectscript::os_str_expr(doc);
    let is_class = doc.to_ascii_lowercase().ends_with(".cls");
    let body = if is_class {
        format!(
            "Set sc=##class(%Compiler.UDL.TextServices).GetTextAsString($Namespace,$Piece(d,\".\",1,$Length(d,\".\")-1),.t) \
             If sc=1 {{ Write !,\"{MARK}-BEGIN~\",!,t,!,\"{MARK}-END~\",! }} \
             Else {{ Write !,\"{MARK}-ERROR~ \",$System.Status.GetErrorText(sc),! }}"
        )
    } else {
        format!(
            "Set r=##class(%Routine).%OpenId(d) \
             If '$IsObject(r) {{ Write !,\"{MARK}-ERROR~ the routine could not be opened\",! }} \
             Else {{ Write !,\"{MARK}-BEGIN~\",! While 'r.AtEnd {{ Write r.ReadLine(),! }} Write \"{MARK}-END~\",! }}"
        )
    };
    format!(
        "Set d={doc_expr} \
         If '##class(%Library.RoutineMgr).Exists(d) {{ Write !,\"{MARK}-MISSING~\",! }} \
         Else {{ {body} }}"
    )
}

/// Read the reply to [`read_exec`]'s code. Public so the unit target can pin the reply format.
pub fn parse_read(output: &str) -> Stored {
    const BEGIN: &str = "~IAD-BEGIN~";
    if let Some(at) = output.find(BEGIN) {
        let after = &output[at + BEGIN.len()..];
        let after = after
            .strip_prefix("\r\n")
            .or_else(|| after.strip_prefix('\n'))
            .unwrap_or(after);
        let Some(end) = after.rfind("~IAD-END~") else {
            return Stored::Unreadable("the reply was cut off before the source ended".to_string());
        };
        let source = after[..end].trim_end_matches(['\r', '\n']);
        return Stored::Found(source.replace("\r\n", "\n"));
    }
    if output.contains("~IAD-MISSING~") {
        return Stored::Missing;
    }
    if let Some(text) = error_text(output) {
        return Stored::Unreadable(text);
    }
    Stored::Unreadable(format!(
        "IRIS gave no source and no reason: {}",
        reply_head(output)
    ))
}

fn error_text(output: &str) -> Option<String> {
    const ERROR: &str = "~IAD-ERROR~";
    let at = output.find(ERROR)?;
    let line = output[at + ERROR.len()..].lines().next().unwrap_or("");
    Some(line.trim().to_string())
}

fn reply_head(output: &str) -> String {
    output.trim().chars().take(160).collect()
}

pub async fn read_exec(
    iris: &IrisConnection,
    ssh_host: Option<&str>,
    namespace: &str,
    doc: &str,
) -> Stored {
    match exec(iris, ssh_host, namespace, &read_code(doc)).await {
        Ok(output) => parse_read(&output),
        Err(msg) => Stored::Transport(msg),
    }
}

/// ObjectScript that lists the documents `pattern` names, one `~IAD-NAME~` line each, then `~IAD-DONE~`.
pub fn list_code(pattern: &str) -> String {
    let pattern_expr = crate::objectscript::os_str_expr(pattern);
    format!(
        "Set rs=##class(%ResultSet).%New(\"%Library.RoutineMgr:StudioOpenDialog\") \
         Set sc=rs.Execute({pattern_expr},1,1,1,1,0,0) \
         If sc'=1 {{ Write !,\"{MARK}-ERROR~ \",$System.Status.GetErrorText(sc),! }} \
         Else {{ While rs.Next() {{ Write !,\"{MARK}-NAME~ \",rs.Get(\"Name\"),! }} Write !,\"{MARK}-DONE~\",! }}"
    )
}

/// What stops a wildcard from being listed.
pub enum ListFailure {
    Transport(String),
    Unreadable(String),
}

/// The documents `pattern` names, through `iris session`. A pattern with no `*` names itself.
pub async fn expand_exec(
    iris: &IrisConnection,
    ssh_host: Option<&str>,
    namespace: &str,
    pattern: &str,
) -> Result<Vec<String>, ListFailure> {
    if !pattern.contains('*') {
        return Ok(vec![doc_name_for(pattern)]);
    }
    let output = exec(iris, ssh_host, namespace, &list_code(pattern))
        .await
        .map_err(ListFailure::Transport)?;
    if let Some(text) = error_text(&output) {
        return Err(ListFailure::Unreadable(text));
    }
    if !output.contains("~IAD-DONE~") {
        return Err(ListFailure::Unreadable(format!(
            "IRIS gave no listing: {}",
            reply_head(&output)
        )));
    }
    Ok(output
        .lines()
        .filter_map(|l| l.trim().strip_prefix("~IAD-NAME~ "))
        .map(|n| n.trim().to_string())
        .collect())
}

/// The documents a docker-only `iris_compile` target names: the target split, wildcard members
/// listed. `Err` is the outcome to report when the exec channel or the listing failed.
pub async fn expand_exec_target(
    iris: &IrisConnection,
    ssh_host: Option<&str>,
    namespace: &str,
    target: &str,
) -> Result<Vec<String>, Outcome> {
    let mut docs = Vec::new();
    for part in split_targets(target) {
        match expand_exec(iris, ssh_host, namespace, &part).await {
            Ok(found) => docs.extend(found),
            Err(ListFailure::Transport(msg)) => return Err(Outcome::Transport(msg)),
            Err(ListFailure::Unreadable(reason)) => {
                return Err(Outcome::Refused(unreadable_listing(&part, &reason)))
            }
        }
    }
    Ok(docs)
}

/// The whole stored-source check for a docker-only `iris_compile`, over documents already listed:
/// read and scan each document and its includes. `Outcome::Transport` means the exec channel
/// itself failed, so the caller can report it exactly as the compile would have.
pub async fn check_exec_docs(
    iris: &IrisConnection,
    ssh_host: Option<&str>,
    namespace: &str,
    docs: Vec<String>,
) -> Outcome {
    check_documents(
        |doc| async move { read_exec(iris, ssh_host, namespace, &doc).await },
        docs.into_iter().map(Seed::Stored).collect(),
    )
    .await
}

/// [`expand_exec_target`] then [`check_exec_docs`].
pub async fn check_exec_target(
    iris: &IrisConnection,
    ssh_host: Option<&str>,
    namespace: &str,
    target: &str,
) -> Outcome {
    match expand_exec_target(iris, ssh_host, namespace, target).await {
        Ok(docs) => check_exec_docs(iris, ssh_host, namespace, docs).await,
        Err(outcome) => outcome,
    }
}
