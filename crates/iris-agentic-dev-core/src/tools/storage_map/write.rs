//! Write lifecycle for SQL storage maps: the scratch copy now; the real PUT, read-back, compile
//! and restore later (plan "Write path").

use super::tree::{emit_body, normalize, parse_body, sort_key, split_class, Node, StorageSpan};
use crate::iris::connection::IrisConnection;
use serde_json::{json, Value};

/// Class keywords the map can depend on; every other keyword in the real header is dropped.
/// `SqlTableName` and `StorageStrategy` are written by [`scratch_source`] itself.
const KEPT_CLASS_KEYWORDS: [&str; 2] = ["SqlRowIdName", "SqlRowIdPrivate"];

/// Members whose body is a `{ … }` block closed by a `}` line; skipped whole.
const BRACED_MEMBERS: [&str; 7] = [
    "Method ",
    "ClassMethod ",
    "ClientMethod ",
    "Query ",
    "XData ",
    "Trigger ",
    "Storage ",
];

/// The scratch copy of `real_text` with Storage body `storage` (FR-015a): `Class <scratch>
/// Extends <same superclasses>`, the `Import` lines, every property, the indices the map names,
/// `SqlRowIdName`/`SqlRowIdPrivate` if the class has them, `SqlTableName` set to the scratch
/// name, `StorageStrategy` naming the block, and the emitted Storage block. No method, query,
/// XData, parameter, projection, trigger or include from the class text.
///
/// Not carried: a `Relationship` (its inverse would name the real class), so a child map's
/// scratch compile fails on the missing field.
pub fn scratch_source(
    real_text: &str,
    storage: &[Node],
    scratch_class: &str,
) -> Result<String, String> {
    if !scratch_class.starts_with("IrisDevTmp.")
        || crate::objectscript::validate_class_name(scratch_class).is_err()
    {
        return Err(format!(
            "{scratch_class} is not a class name in the IrisDevTmp package"
        ));
    }
    let sql_maps = sql_map_names(storage);
    if sql_maps.is_empty() {
        // FR-002: default storage is read-only here. IRIS also refuses a second class on its
        // globals (#5564, measured), so a copy could not compile anyway.
        return Err(
            "the Storage block has no SQLMap: default storage is read-only in \
                    iris_storage_map, and has no scratch copy"
                .into(),
        );
    }
    let lines: Vec<&str> = real_text.lines().collect();
    let Some(header_at) = lines.iter().position(|l| l.starts_with("Class ")) else {
        return Err("the class text has no Class line".into());
    };
    let span = split_class(real_text)
        .map_err(|e| format!("cannot split the class text: {}", e.reason))?
        .into_iter()
        .next()
        .ok_or("the class text has no Storage block")?;

    let (supers, keywords) = parse_header(lines[header_at]);
    let short = scratch_class.rsplit('.').next().unwrap_or(scratch_class);
    let mut kws: Vec<String> = keywords
        .into_iter()
        .filter(|k| KEPT_CLASS_KEYWORDS.contains(&keyword_name(k)))
        .collect();
    kws.push(format!("SqlTableName = {short}"));
    kws.push(format!("StorageStrategy = {}", span.name));
    kws.sort_by(|a, b| keyword_name(a).cmp(keyword_name(b)));

    let (properties, indices) = members(&lines[header_at + 1..]);
    let kept_indices: Vec<&Member> = indices
        .iter()
        .filter(|i| sql_maps.contains(&i.name.as_str()) || is_idkey(&i.text))
        .collect();
    // Every property, mapped or not: the real compile refuses a property no data map files
    // (#5502, measured 2026-11-11), so the copy has to see it too.
    let kept_properties = properties.iter();

    let mut out = String::new();
    for import in lines[..header_at]
        .iter()
        .filter(|l| l.starts_with("Import "))
    {
        out.push_str(import);
        out.push_str("\n\n");
    }
    out.push_str(&format!("Class {scratch_class}"));
    if let Some(s) = supers {
        out.push_str(&format!(" Extends {s}"));
    }
    out.push_str(&format!(" [ {} ]\n{{\n\n", kws.join(", ")));
    for m in kept_properties.chain(kept_indices) {
        out.push_str(&m.text);
        out.push_str("\n\n");
    }
    out.push_str(&format!(
        "Storage {}\n{{\n{}\n}}\n\n}}\n",
        span.name,
        emit_body(storage)
    ));
    Ok(out)
}

struct Member {
    name: String,
    text: String,
}

/// The superclass list (as written) and the keyword list of a `Class` line.
fn parse_header(line: &str) -> (Option<String>, Vec<String>) {
    let (head, kw_text) = match top_level_find(line, '[') {
        Some(at) => {
            let close = line.rfind(']').unwrap_or(line.len());
            (&line[..at], line.get(at + 1..close).unwrap_or(""))
        }
        None => (line, ""),
    };
    let supers = head
        .split_once(" Extends ")
        .map(|(_, s)| s.trim().to_string())
        .filter(|s| !s.is_empty());
    let keywords = split_top_level(kw_text, ',')
        .into_iter()
        .map(|k| k.trim().to_string())
        .filter(|k| !k.is_empty())
        .collect();
    (supers, keywords)
}

/// `SqlTableName` of `SqlTableName = X`, `Final` of `Final`.
fn keyword_name(k: &str) -> &str {
    k.split(|c: char| c == '=' || c.is_whitespace())
        .next()
        .unwrap_or(k)
}

/// The class body's `Property` and `Index` declarations, each with its full text. A braced
/// member is skipped to its closing `}` line, so nothing inside a method or query is read as a
/// declaration.
fn members(body: &[&str]) -> (Vec<Member>, Vec<Member>) {
    let (mut properties, mut indices) = (Vec::new(), Vec::new());
    let mut i = 0;
    while i < body.len() {
        let line = body[i];
        if BRACED_MEMBERS.iter().any(|k| line.starts_with(k)) {
            i += 1;
            while i < body.len() && body[i] != "}" {
                i += 1;
            }
            i += 1;
            continue;
        }
        let into = if line.starts_with("Property ") {
            &mut properties
        } else if line.starts_with("Index ") {
            &mut indices
        } else {
            i += 1;
            continue;
        };
        let start = i;
        let mut depth: i32 = 0;
        loop {
            for c in body[i].chars() {
                match c {
                    '{' | '(' => depth += 1,
                    '}' | ')' => depth -= 1,
                    _ => {}
                }
            }
            if (depth <= 0 && body[i].trim_end().ends_with(';')) || i + 1 >= body.len() {
                break;
            }
            i += 1;
        }
        let text = body[start..=i].join("\n");
        let name = line
            .split_whitespace()
            .nth(1)
            .unwrap_or_default()
            .trim_end_matches(';')
            .to_string();
        into.push(Member { name, text });
        i += 1;
    }
    (properties, indices)
}

/// The `name` of every top-level `SQLMap`.
fn sql_map_names(nodes: &[Node]) -> Vec<&str> {
    nodes
        .iter()
        .filter(|n| n.tag == "SQLMap")
        .filter_map(|n| n.name.as_deref())
        .collect()
}

/// The `IdKey` keyword in an `Index` declaration's `[ … ]`.
fn is_idkey(text: &str) -> bool {
    text.find('[').is_some_and(|at| {
        split_top_level(&text[at + 1..], ',')
            .iter()
            .any(|k| keyword_name(k.trim().trim_end_matches([']', ';', ' '])) == "IdKey")
    })
}

/// The first `c` outside quotes, parentheses and braces.
fn top_level_find(s: &str, c: char) -> Option<usize> {
    let (mut depth, mut quoted) = (0i32, false);
    for (i, ch) in s.char_indices() {
        match ch {
            '"' => quoted = !quoted,
            '(' | '{' if !quoted => depth += 1,
            ')' | '}' if !quoted => depth -= 1,
            _ if ch == c && !quoted && depth == 0 => return Some(i),
            _ => {}
        }
    }
    None
}

/// `s` split on `sep` outside quotes, parentheses and braces.
fn split_top_level(s: &str, sep: char) -> Vec<&str> {
    let mut out = Vec::new();
    let mut rest = s;
    while let Some(at) = top_level_find(rest, sep) {
        out.push(&rest[..at]);
        rest = &rest[at + sep.len_utf8()..];
    }
    out.push(rest);
    out
}

// ───────────────────────────────────────────────────────────────────────────
// The real write: new class text and the read-back check (T071)
// ───────────────────────────────────────────────────────────────────────────

/// The storage-level Description of a tree, if any.
fn description(nodes: &[Node]) -> Option<&str> {
    nodes
        .iter()
        .find(|n| n.tag == "Description" && n.is_leaf())
        .and_then(|n| n.value.as_deref())
}

/// `original` with the Storage block at `span` written from `nodes` in IRIS's canonical form
/// (R3): only the body changes. When the storage Description changes, the `///` lines IRIS
/// writes above `Storage <name>` change with it (R3.3).
pub fn new_class_text(original: &str, span: &StorageSpan, nodes: &[Node]) -> String {
    let body = emit_body(&normalize(nodes.to_vec()));
    let suffix = &original[span.body_end..];
    let old = parse_body(&original[span.body_start..span.body_end]).ok();
    let old_description = old.as_deref().and_then(description);
    if old_description == description(nodes) {
        return format!("{}{body}{suffix}", &original[..span.body_start]);
    }
    // Drop the `///` lines directly above `Storage <name>`; the new ones are written below.
    let mut head = &original[..span.start];
    loop {
        let trimmed = head.strip_suffix('\n').unwrap_or(head);
        let line_start = trimmed.rfind('\n').map_or(0, |i| i + 1);
        if trimmed.len() > line_start && trimmed[line_start..].starts_with("///") {
            head = &head[..line_start];
        } else {
            break;
        }
    }
    let comment: String = description(nodes)
        .map(|d| d.lines().map(|l| format!("/// {l}\n")).collect())
        .unwrap_or_default();
    format!(
        "{head}{comment}{}{body}{suffix}",
        &original[span.start..span.body_start]
    )
}

/// What the read-back found that differs from what was sent (R9): elements IRIS dropped, and
/// any other difference, which R3's rules do not explain and so is a writer bug.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ReadbackDiff {
    pub dropped: Vec<String>,
    pub unexplained: Vec<String>,
}

/// Compare the tree iad sent with the one IRIS stored, after R3's rules (`normalize`: empty
/// leaves, `BlockCount 0` and `ConditionalWithHostVars 0` dropped). Elements are matched by tag
/// and name; children out of canonical order are unexplained, since IRIS's form is a fixed point.
pub fn readback_diff(sent: &[Node], stored: &[Node]) -> ReadbackDiff {
    let mut d = ReadbackDiff::default();
    compare(&normalize(sent.to_vec()), stored, "", &mut d);
    d
}

/// `Tag[name]`, `Tag`, with `#k` for the k-th repeat of the same key.
fn keyed(nodes: &[Node]) -> Vec<(String, &Node)> {
    let mut seen: Vec<String> = Vec::new();
    nodes
        .iter()
        .map(|n| {
            let base = match &n.name {
                Some(x) => format!("{}[{x}]", n.tag),
                None => n.tag.clone(),
            };
            let k = seen.iter().filter(|s| **s == base).count();
            seen.push(base.clone());
            (if k == 0 { base } else { format!("{base}#{k}") }, n)
        })
        .collect()
}

fn compare(sent: &[Node], stored: &[Node], at: &str, d: &mut ReadbackDiff) {
    let mut canonical = stored.to_vec();
    canonical.sort_by_key(sort_key);
    if canonical != stored {
        d.unexplained.push(format!(
            "{}: children are not in IRIS's order",
            if at.is_empty() { "/" } else { at }
        ));
    }
    let sent = keyed(sent);
    let stored = keyed(stored);
    for (k, s) in &sent {
        let here = format!("{at}/{k}");
        match stored.iter().find(|(sk, _)| sk == k) {
            None => d.dropped.push(here),
            Some((_, t)) => match (&s.children, &t.children) {
                (Some(sc), Some(tc)) => compare(sc, tc, &here, d),
                (None, None) if s.value != t.value => d.unexplained.push(format!(
                    "{here}: sent {:?}, IRIS has {:?}",
                    s.value.as_deref().unwrap_or(""),
                    t.value.as_deref().unwrap_or("")
                )),
                (None, None) => {}
                _ => d
                    .unexplained
                    .push(format!("{here}: sent as one shape, IRIS has another")),
            },
        }
    }
    for (k, _) in &stored {
        if !sent.iter().any(|(sk, _)| sk == k) {
            d.unexplained
                .push(format!("{at}/{k}: IRIS has it, iad did not send it"));
        }
    }
}

// ───────────────────────────────────────────────────────────────────────────
// The write path (T072, T073): plan "Write path", in order
// ───────────────────────────────────────────────────────────────────────────

/// An error answer: `success: false`, the code and message, and `extra`'s keys.
pub(super) fn body(code: &str, msg: String, extra: Value) -> Value {
    let mut b = json!({"success": false, "error_code": code, "error": msg});
    if let (Some(o), Value::Object(e)) = (b.as_object_mut(), extra) {
        o.extend(e);
    }
    b
}

/// The answer for a tree that breaks FR-014 rules: `STORAGE_MAP_READ_ONLY` when one of them is
/// storage iad only reads, else `STORAGE_MAP_INVALID`, with every rule and its path.
pub fn invalid_body(found: &[super::validate::Invalid]) -> Value {
    let code = if found.iter().any(|i| i.code == "STORAGE_MAP_READ_ONLY") {
        "STORAGE_MAP_READ_ONLY"
    } else {
        "STORAGE_MAP_INVALID"
    };
    let listed: Vec<String> = found
        .iter()
        .map(|i| format!("{} at {}", i.rule, i.path))
        .collect();
    let rules: Vec<Value> = found
        .iter()
        .map(|i| json!({"path": i.path, "rule": i.rule}))
        .collect();
    body(
        code,
        format!(
            "the map breaks {} rule(s): {}",
            found.len(),
            listed.join("; ")
        ),
        json!({"rules": rules}),
    )
}

/// The compile-time gate on `text` as document `doc`, when the gate is on for the call's server
/// (fails closed outside a call).
fn compile_time_refusal(text: &str, doc: &str) -> Option<Value> {
    let on = crate::tools::target::compile_gate().map_or(true, |g| g.on);
    if on {
        crate::policy::code_edit_gate::check_compile_time_code_mode(text, doc)
    } else {
        None
    }
}

/// Compile `nodes` in a scratch copy of `text` and delete it (FR-015, FR-015a). `Ok` is the
/// scratch class's name; `Err` is the answer: the copy could not be built, the compile-time gate
/// refused it, it could not be sent, or it did not compile (`STORAGE_MAP_COMPILE_FAILED`).
pub async fn scratch_check(
    iris: &IrisConnection,
    client: &reqwest::Client,
    text: &str,
    nodes: &[Node],
    ns: &str,
    class: &str,
) -> Result<String, Value> {
    // Build the copy once under a stand-in name, so a failure or a compile-time refusal comes
    // back before any request. The copy under the real name differs only in the name.
    let probe = scratch_source(text, nodes, "IrisDevTmp.SmCheck")
        .map_err(|e| body("STORAGE_MAP_INVALID", e, json!({})))?;
    if let Some(refusal) = compile_time_refusal(&probe, "IrisDevTmp.SmCheck.cls") {
        return Err(refusal);
    }
    let (scratch, result) = iris
        .compile_scratch_class(
            |name| scratch_source(text, nodes, name).unwrap_or_default(),
            ns,
            client,
            "Sm",
        )
        .await
        .map_err(|e| {
            body(
                "UPLOAD_FAILED",
                format!("the scratch copy could not be sent to IRIS: {e}"),
                json!({}),
            )
        })?;
    if !result.success() {
        return Err(body(
            "STORAGE_MAP_COMPILE_FAILED",
            format!(
                "the map does not compile ({} error(s)); the real class {class} was not touched",
                result.errors.len()
            ),
            json!({
                "scratch_class": scratch,
                "iris_errors": result.errors,
                "explanations": super::explain::explanations(&result.errors),
            }),
        ));
    }
    Ok(scratch)
}

/// The document's text and timestamp (`result.ts`), or `None` when IRIS has no such document.
pub async fn get_doc(
    iris: &IrisConnection,
    client: &reqwest::Client,
    doc: &str,
    ns: &str,
) -> anyhow::Result<Option<(String, String)>> {
    let url = iris.versioned_ns_url(ns, &format!("/doc/{}", urlencoding::encode(doc)));
    let resp = client
        .get(&url)
        .basic_auth(&iris.username, Some(&iris.password))
        .send()
        .await?;
    if resp.status() == reqwest::StatusCode::NOT_FOUND {
        return Ok(None);
    }
    if !resp.status().is_success() {
        anyhow::bail!("GET {doc}: HTTP {}", resp.status());
    }
    let b: Value = crate::iris::http::json_or_default(resp).await;
    let lines: Vec<&str> = b["result"]["content"]
        .as_array()
        .map(|a| a.iter().filter_map(Value::as_str).collect())
        .unwrap_or_default();
    let ts = b["result"]["ts"].as_str().unwrap_or_default().to_string();
    if lines.is_empty() || ts.is_empty() {
        anyhow::bail!("GET {doc}: no content or timestamp in the answer");
    }
    Ok(Some((lines.join("\n"), ts)))
}

/// PUT `text` as `doc` only if IRIS still holds the version stamped `expected_ts` (FR-010a). No
/// `ignoreConflict`: IRIS compares `If-None-Match` with its own timestamp and answers 409 on a
/// change (research R8, measured). `Ok` is the new timestamp. An `Err` body carries `stored:
/// true` when IRIS kept the text although it reported an error, so the caller restores.
pub async fn guarded_put(
    iris: &IrisConnection,
    client: &reqwest::Client,
    doc: &str,
    text: &str,
    ns: &str,
    expected_ts: &str,
) -> Result<String, Value> {
    let url = iris.versioned_ns_url(ns, &format!("/doc/{}", urlencoding::encode(doc)));
    let lines: Vec<&str> = text.split('\n').collect();
    let resp = client
        .put(&url)
        .header(
            reqwest::header::USER_AGENT,
            crate::iris::connection::user_agent(crate::iris::connection::caller_mode()),
        )
        .header(reqwest::header::IF_NONE_MATCH, expected_ts)
        .basic_auth(&iris.username, Some(&iris.password))
        .json(&json!({"enc": false, "content": lines}))
        .send()
        .await
        .map_err(|e| {
            body(
                "UPLOAD_FAILED",
                format!("PUT {doc}: {e}"),
                json!({"stored": "unknown"}),
            )
        })?;
    let status = resp.status();
    let etag = resp
        .headers()
        .get(reqwest::header::ETAG)
        .and_then(|v| v.to_str().ok())
        .map(str::to_string);
    let b: Value = crate::iris::http::json_or_default(resp).await;
    if status == reqwest::StatusCode::CONFLICT {
        return Err(body(
            "STORAGE_MAP_VERSION_CONFLICT",
            format!("{doc} changed after iad read it; nothing was written. Read it again."),
            json!({"current_ts": b["result"]["ts"], "stored": false}),
        ));
    }
    if !status.is_success() {
        return Err(body(
            "UPLOAD_FAILED",
            format!("PUT {doc}: HTTP {status}"),
            json!({"stored": false}),
        ));
    }
    if let Some(err) = crate::iris::connection::check_atelier_put_errors(&b) {
        return Err(body("UPLOAD_FAILED", err, json!({"stored": true})));
    }
    Ok(etag
        .or_else(|| b["result"]["ts"].as_str().map(str::to_string))
        .unwrap_or_default())
}

/// What a write needs from the call: where, and how the gates judge it.
pub struct WriteCtx<'a> {
    pub iris: &'a IrisConnection,
    pub client: &'a reqwest::Client,
    pub class: &'a str,
    pub ns: &'a str,
    pub shape: &'a super::CallShape,
    pub policy: &'a crate::policy::effective::EffectivePolicy,
    pub server: &'a str,
}

/// The class as iad read it before the write: its text and timestamp, the Storage span, and the
/// tree of that span.
pub struct Loaded {
    pub text: String,
    pub ts: String,
    pub span: StorageSpan,
    pub nodes: Vec<Node>,
}

/// The storage tree of a stored class text, if it parses.
fn stored_tree(text: &str) -> Option<Vec<Node>> {
    super::storage_tree(text)
        .ok()
        .flatten()
        .map(|(_, nodes)| nodes)
}

/// Put the text iad read back and compile it (R8). `Ok` when the class is as it was.
async fn restore(w: &WriteCtx<'_>, loaded: &Loaded) -> Result<(), String> {
    let doc = format!("{}.cls", w.class);
    let (_, ts) = get_doc(w.iris, w.client, &doc, w.ns)
        .await
        .map_err(|e| e.to_string())?
        .ok_or("the class is gone")?;
    guarded_put(w.iris, w.client, &doc, &loaded.text, w.ns, &ts)
        .await
        .map_err(|b| b["error"].as_str().unwrap_or("PUT failed").to_string())?;
    let compiled = w
        .iris
        .compile_document(&doc, w.ns, "cuk", w.client)
        .await
        .map_err(|e| e.to_string())?;
    if compiled.success() {
        Ok(())
    } else {
        Err(compiled.errors.join("; "))
    }
}

/// `err` after a failed step that left new text on the server: restore, and say what came of it.
async fn undo(w: &WriteCtx<'_>, loaded: &Loaded, mut err: Value) -> Value {
    match restore(w, loaded).await {
        Ok(()) => err["restored"] = json!(true),
        Err(e) => {
            err["restored"] = json!(false);
            err["restore_error"] = json!(e);
            err["error"] = json!(format!(
                "the previous version could NOT be restored ({e}); {}",
                err["error"].as_str().unwrap_or("")
            ));
        }
    }
    err
}

/// Write `after` in place of `loaded`'s Storage block (plan "Write path"): validate, gates,
/// scratch compile, guarded PUT, read-back with the gates again on what IRIS stored (FR-012a),
/// real compile, restore on failure. `Ok` holds the new storage model and versions; `Err` is the
/// answer, which says whether anything was written and whether it was undone.
pub async fn write_map(
    w: &WriteCtx<'_>,
    loaded: &Loaded,
    after: Vec<Node>,
) -> Result<Value, Value> {
    if let Err(found) = super::validate::validate_change(&loaded.nodes, &after) {
        return Err(invalid_body(&found));
    }
    if let Some(refusal) = super::code::gate_tree(&after, w.shape, w.policy, w.server) {
        return Err(refusal);
    }
    // Data checks (FR-023) arrive in Phase 10 (T093).
    let scratch = scratch_check(w.iris, w.client, &loaded.text, &after, w.ns, w.class).await?;

    let doc = format!("{}.cls", w.class);
    let new_text = new_class_text(&loaded.text, &loaded.span, &after);
    if let Some(refusal) = compile_time_refusal(&new_text, &doc) {
        return Err(refusal);
    }
    if let Err(e) = guarded_put(w.iris, w.client, &doc, &new_text, w.ns, &loaded.ts).await {
        return Err(if e["stored"] == true {
            undo(w, loaded, e).await
        } else {
            e
        });
    }

    // Read back what IRIS stored, before the compile, and judge that (FR-012, FR-012a).
    let stored = match get_doc(w.iris, w.client, &doc, w.ns).await {
        Ok(Some((text, _))) => text,
        Ok(None) => {
            return Err(body(
                "STORAGE_MAP_WRITER_MISMATCH",
                format!("{doc} is gone after the PUT"),
                json!({}),
            ))
        }
        Err(e) => {
            let err = body("UPLOAD_FAILED", format!("read-back: {e}"), json!({}));
            return Err(undo(w, loaded, err).await);
        }
    };
    let Some(stored_nodes) = stored_tree(&stored) else {
        let err = body(
            "STORAGE_MAP_WRITER_MISMATCH",
            format!("IRIS stored a Storage block iad cannot parse in {doc}"),
            json!({}),
        );
        return Err(undo(w, loaded, err).await);
    };
    if let Some(refusal) = super::code::gate_tree(&stored_nodes, w.shape, w.policy, w.server) {
        return Err(undo(w, loaded, refusal).await);
    }
    let diff = readback_diff(&after, &stored_nodes);
    if !diff.dropped.is_empty() {
        let err = body(
            "STORAGE_MAP_DROPPED",
            format!("IRIS dropped {} element(s) iad sent", diff.dropped.len()),
            json!({"dropped": diff.dropped}),
        );
        return Err(undo(w, loaded, err).await);
    }
    if !diff.unexplained.is_empty() {
        let err = body(
            "STORAGE_MAP_WRITER_MISMATCH",
            format!(
                "IRIS stored a different map than iad sent ({} difference(s)); this is an iad bug",
                diff.unexplained.len()
            ),
            json!({"differences": diff.unexplained}),
        );
        return Err(undo(w, loaded, err).await);
    }

    let compiled = match w.iris.compile_document(&doc, w.ns, "cuk", w.client).await {
        Ok(c) => c,
        Err(e) => {
            let err = body("UPLOAD_FAILED", format!("compile: {e}"), json!({}));
            return Err(undo(w, loaded, err).await);
        }
    };
    if !compiled.success() {
        let errors = compiled.errors;
        let explanations = super::explain::explanations(&errors);
        return Err(match restore(w, loaded).await {
            Ok(()) => body(
                "STORAGE_MAP_RESTORED",
                format!(
                    "{} did not compile with the new map ({} error(s)); the previous version was put \
                     back and compiled",
                    w.class,
                    errors.len()
                ),
                json!({"iris_errors": errors, "explanations": explanations, "restored": true}),
            ),
            Err(e) => body(
                "STORAGE_MAP_RESTORE_FAILED",
                format!(
                    "{} did not compile with the new map, and the previous version could not be \
                     restored: {e}",
                    w.class
                ),
                json!({"iris_errors": errors, "explanations": explanations, "restored": false,
                       "restore_error": e}),
            ),
        });
    }

    // The final text (a compile may add a StreamLocation, R3.7) gives the model the caller sees.
    let (final_text, _) = get_doc(w.iris, w.client, &doc, w.ns)
        .await
        .ok()
        .flatten()
        .unwrap_or((stored, String::new()));
    let final_nodes = stored_tree(&final_text).unwrap_or(stored_nodes);
    let before = super::model::from_tree(w.class, &loaded.span.name, &loaded.nodes);
    let now = super::model_with_facts(w.class, &loaded.span.name, &final_nodes);
    Ok(json!({
        "storage": now.storage,
        "derived": now.derived,
        "versions": {"before": before.storage.version, "after": now.storage.version},
        "code_changed": super::code::code_changed(&loaded.nodes, &after),
        "scratch_class": scratch,
        "warnings": [],
    }))
}

/// The audit record of one map write (FR-030c): what changed, by path, and whether map code
/// changed. Paths and versions only, never a value, so no code text reaches the log.
pub fn map_write_record(
    class: &str,
    map: Option<&str>,
    version_before: &str,
    version_after: &str,
    changed: &[String],
    code_changed: bool,
) -> Value {
    json!({
        "class": class,
        "map": map,
        "version_before": version_before,
        "version_after": version_after,
        "changed_paths": changed,
        "code_changed": code_changed,
    })
}
