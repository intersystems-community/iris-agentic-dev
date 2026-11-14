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
    scratch_probe(text, nodes)?;
    let untouched = format!("the real class {class} was not touched");
    let (scratch, _) = scratch_compile(iris, client, text, nodes, ns, None, &untouched).await?;
    Ok(scratch)
}

/// Build the scratch copy once under a stand-in name, so a failure (`STORAGE_MAP_INVALID`) or a
/// compile-time refusal comes back before any request. The copy under the real name differs only
/// in the name.
pub fn scratch_probe(text: &str, nodes: &[Node]) -> Result<(), Value> {
    let probe = scratch_source(text, nodes, "IrisDevTmp.SmCheck")
        .map_err(|e| body("STORAGE_MAP_INVALID", e, json!({})))?;
    match compile_time_refusal(&probe, "IrisDevTmp.SmCheck.cls") {
        Some(refusal) => Err(refusal),
        None => Ok(()),
    }
}

/// Compile `nodes` in a scratch copy of `text` on `iris`, run `query` against it when it
/// compiled, and delete it. `Ok` is the scratch class's name and the query's answer; `Err` is
/// `UPLOAD_FAILED` or `STORAGE_MAP_COMPILE_FAILED`, whose message ends with `untouched`.
pub async fn scratch_compile(
    iris: &IrisConnection,
    client: &reqwest::Client,
    text: &str,
    nodes: &[Node],
    ns: &str,
    query: Option<&(dyn Fn(&str) -> String + Sync)>,
    untouched: &str,
) -> Result<(String, Option<anyhow::Result<Value>>), Value> {
    let (scratch, result, answer) = iris
        .compile_scratch_class_then(
            |name| scratch_source(text, nodes, name).unwrap_or_default(),
            ns,
            client,
            "Sm",
            query,
        )
        .await
        .map_err(
            |e| match e.downcast_ref::<crate::iris::connection::ScratchRefused>() {
                // A gate refused the copy before its compile (the superclass check): its own answer.
                Some(refused) => refused.0.clone(),
                None => body(
                    "UPLOAD_FAILED",
                    format!("the scratch copy could not be sent to IRIS: {e}"),
                    json!({}),
                ),
            },
        )?;
    if !result.success() {
        return Err(body(
            "STORAGE_MAP_COMPILE_FAILED",
            format!(
                "the map does not compile ({} error(s)); {untouched}",
                result.errors.len()
            ),
            json!({
                "scratch_class": scratch,
                "iris_errors": result.errors,
                "explanations": super::explain::explanations(&result.errors),
            }),
        ));
    }
    Ok((scratch, answer))
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
    /// The identity the opt-ins run as (the service account when set, T036); `None` when the
    /// call asks for no opt-in.
    pub exec: Option<(&'a IrisConnection, &'a reqwest::Client)>,
}

/// The class as iad read it before the write: its text and timestamp, the Storage span, and the
/// tree of that span.
pub struct Loaded {
    pub text: String,
    pub ts: String,
    pub span: StorageSpan,
    pub nodes: Vec<Node>,
}

/// Run `gate` on every Storage block of `text` (FR-012a: every block, not only the first): the
/// first refusal, or `STORAGE_MAP_PARSE_FAILED` naming a block that does not parse, so a block iad
/// cannot read is never let through unjudged.
pub fn gate_blocks(text: &str, mut gate: impl FnMut(&[Node]) -> Option<Value>) -> Option<Value> {
    let spans = match split_class(text) {
        Ok(spans) => spans,
        Err(e) => {
            return Some(body(
                "STORAGE_MAP_PARSE_FAILED",
                format!("cannot find the Storage blocks: {}", e.reason),
                json!({"offset": e.offset, "context": e.context}),
            ))
        }
    };
    for span in spans {
        let nodes = match parse_body(&text[span.body_start..span.body_end]) {
            Ok(nodes) => nodes,
            Err(e) => {
                return Some(body(
                    "STORAGE_MAP_PARSE_FAILED",
                    format!("cannot parse Storage {}: {}", span.name, e.reason),
                    json!({"offset": e.offset, "context": e.context}),
                ))
            }
        };
        if let Some(refusal) = gate(&nodes) {
            return Some(refusal);
        }
    }
    None
}

/// Every Storage block of `text`, by name, parsed.
fn all_blocks(text: &str) -> Result<Vec<(String, Vec<Node>)>, super::tree::ParseError> {
    split_class(text)?
        .into_iter()
        .map(|span| {
            Ok((
                span.name.clone(),
                parse_body(&text[span.body_start..span.body_end])?,
            ))
        })
        .collect()
}

/// Every Storage block of `text` that holds an SQL map, by name (FR-012a: all of them, not only
/// the first). Default storage holds no `SQLMap` and is not one of them.
pub fn map_blocks(text: &str) -> Result<Vec<(String, Vec<Node>)>, super::tree::ParseError> {
    Ok(all_blocks(text)?
        .into_iter()
        .filter(|(_, nodes)| nodes.iter().any(|n| n.tag == "SQLMap"))
        .collect())
}

/// Whether the map code of any map block of `after` differs from the same-named block of
/// `before` (`None`: a new class, whose map code is all new) (FR-027).
pub fn map_code_changed(
    before: Option<&str>,
    after: &str,
) -> Result<bool, super::tree::ParseError> {
    let old = match before {
        Some(t) => all_blocks(t)?,
        None => Vec::new(),
    };
    Ok(map_blocks(after)?.iter().any(|(name, nodes)| {
        let was = old
            .iter()
            .find(|(n, _)| n == name)
            .map_or(&[][..], |(_, n)| n.as_slice());
        super::code::code_changed(was, nodes)
    }))
}

/// What IRIS stored for `doc`, judged against the Storage blocks iad `sent`, by name (FR-012,
/// FR-012a): the stored blocks when they are what was sent, else the answer. `gate` is the call's
/// gates, run again on every block IRIS kept. Both writers judge with this; each decides how to
/// undo.
pub fn judge_stored(
    sent: &[(String, Vec<Node>)],
    stored_text: Option<&str>,
    doc: &str,
    gate: impl FnMut(&[Node]) -> Option<Value>,
) -> Result<Vec<(String, Vec<Node>)>, Value> {
    let unreadable = || {
        body(
            "STORAGE_MAP_WRITER_MISMATCH",
            format!("iad cannot read back the Storage block IRIS stored for {doc}"),
            json!({}),
        )
    };
    let Some(text) = stored_text else {
        return Err(unreadable());
    };
    let stored = match all_blocks(text) {
        Ok(blocks) if !blocks.is_empty() => blocks,
        _ => return Err(unreadable()),
    };
    if let Some(refusal) = gate_blocks(text, gate) {
        return Err(refusal);
    }
    // One block keeps the map's own paths; several say which block each path is in.
    let label = |name: &str, path: String| {
        if sent.len() > 1 {
            format!("Storage {name}{path}")
        } else {
            path
        }
    };
    let mut dropped = Vec::new();
    let mut unexplained = Vec::new();
    let mut kept = Vec::new();
    for (name, nodes) in sent {
        match stored.iter().find(|(n, _)| n == name) {
            None => dropped.push(format!("Storage {name}")),
            Some((_, have)) => {
                let diff = readback_diff(nodes, have);
                dropped.extend(diff.dropped.into_iter().map(|p| label(name, p)));
                unexplained.extend(diff.unexplained.into_iter().map(|p| label(name, p)));
                kept.push((name.clone(), have.clone()));
            }
        }
    }
    if !dropped.is_empty() {
        return Err(body(
            "STORAGE_MAP_DROPPED",
            format!("IRIS dropped {} element(s) iad sent", dropped.len()),
            json!({"dropped": dropped}),
        ));
    }
    if !unexplained.is_empty() {
        return Err(body(
            "STORAGE_MAP_WRITER_MISMATCH",
            format!(
                "IRIS stored a different map than iad sent ({} difference(s)); this is an iad bug",
                unexplained.len()
            ),
            json!({"differences": unexplained}),
        ));
    }
    Ok(kept)
}

/// Put `previous` back as `doc` and compile it, only over the version iad wrote (FR-010a): IRIS
/// must still hold `ours` (what iad's read-back saw, or the text it sent when the read-back
/// failed). A class someone changed since is refused, never overwritten; the guarded PUT on the
/// timestamp read with it covers a change between that read and the PUT. `Ok` when it compiled.
pub async fn restore_over(
    iris: &IrisConnection,
    client: &reqwest::Client,
    doc: &str,
    previous: &str,
    ns: &str,
    ours: &str,
) -> Result<(), String> {
    let (now, ts) = get_doc(iris, client, doc, ns)
        .await
        .map_err(|e| e.to_string())?
        .ok_or("the class is gone")?;
    if now.trim_end() != ours.trim_end() {
        return Err(CHANGED_SINCE.into());
    }
    if let Err(b) = guarded_put(iris, client, doc, previous, ns, &ts).await {
        return Err(if b["error_code"] == "STORAGE_MAP_VERSION_CONFLICT" {
            CHANGED_SINCE.into()
        } else {
            b["error"].as_str().unwrap_or("PUT failed").to_string()
        });
    }
    let compiled = iris
        .compile_document(doc, ns, "cuk", client)
        .await
        .map_err(|e| e.to_string())?;
    if compiled.success() {
        Ok(())
    } else {
        Err(compiled.errors.join("; "))
    }
}

/// Why a restore was refused: the class is no longer the version iad wrote.
pub const CHANGED_SINCE: &str = "the class changed after iad wrote it, so iad did not put the \
     previous version back over that change; read it and decide";

/// Put the text iad read back and compile it (R8), over the version iad wrote only (FR-010a).
async fn restore(w: &WriteCtx<'_>, loaded: &Loaded, ours: &str) -> Result<(), String> {
    let doc = format!("{}.cls", w.class);
    restore_over(w.iris, w.client, &doc, &loaded.text, w.ns, ours).await
}

/// `err` after a failed step that left new text on the server: restore, and say what came of it.
async fn undo(w: &WriteCtx<'_>, loaded: &Loaded, ours: &str, mut err: Value) -> Value {
    match restore(w, loaded, ours).await {
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
    let plan = data_checks(
        &CheckCtx {
            iris: w.iris,
            client: w.client,
            ns: w.ns,
            class: w.class,
            seed_counter: w.shape.seed_counter,
            build_indexes: w.shape.build_indexes,
            warn_only: false,
        },
        &loaded.nodes,
        &after,
    )
    .await?;
    let scratch = scratch_check(w.iris, w.client, &loaded.text, &after, w.ns, w.class).await?;

    let doc = format!("{}.cls", w.class);
    let new_text = new_class_text(&loaded.text, &loaded.span, &after);
    // FR-012a: the other Storage blocks go out in the same PUT, so they are judged too.
    if let Some(refusal) = gate_blocks(&new_text, |nodes| {
        super::code::gate_tree(nodes, w.shape, w.policy, w.server)
    }) {
        return Err(refusal);
    }
    if let Some(refusal) = compile_time_refusal(&new_text, &doc) {
        return Err(refusal);
    }
    if let Err(e) = guarded_put(w.iris, w.client, &doc, &new_text, w.ns, &loaded.ts).await {
        return Err(if e["stored"] == true {
            undo(w, loaded, &new_text, e).await
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
            return Err(undo(w, loaded, &new_text, err).await);
        }
    };
    let sent = [(loaded.span.name.clone(), after.clone())];
    let stored_nodes = match judge_stored(&sent, Some(&stored), &doc, |nodes| {
        super::code::gate_tree(nodes, w.shape, w.policy, w.server)
    }) {
        Ok(mut blocks) => blocks.pop().map(|(_, nodes)| nodes).unwrap_or_default(),
        Err(err) => return Err(undo(w, loaded, &stored, err).await),
    };

    let compiled = match w.iris.compile_document(&doc, w.ns, "cuk", w.client).await {
        Ok(c) => c,
        Err(e) => {
            let err = body("UPLOAD_FAILED", format!("compile: {e}"), json!({}));
            return Err(undo(w, loaded, &stored, err).await);
        }
    };
    if !compiled.success() {
        let errors = compiled.errors;
        let explanations = super::explain::explanations(&errors);
        return Err(match restore(w, loaded, &stored).await {
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

    let opt_ins_run = run_opt_ins(w, &plan).await;
    let mut warnings = plan.warnings.clone();

    // The final text (a compile may add a StreamLocation, R3.7) gives the model the caller sees.
    let final_text =
        match get_doc(w.iris, w.client, &doc, w.ns).await {
            Ok(Some((text, _))) => text,
            other => {
                warnings.push(format!(
                "the class could not be read after its compile ({}), so the model shown is the \
                 one iad wrote",
                other.err().map_or("it is gone".to_string(), |e| e.to_string())
            ));
                stored
            }
        };
    let final_nodes = super::storage_tree(&final_text)
        .ok()
        .flatten()
        .map(|(_, nodes)| nodes)
        .unwrap_or(stored_nodes);
    let before = super::model::from_tree(w.class, &loaded.span.name, &loaded.nodes);
    let now = super::model_with_facts(w.class, &loaded.span.name, &final_nodes);
    let answer = json!({
        "storage": now.storage,
        "derived": now.derived,
        "versions": {"before": before.storage.version, "after": now.storage.version},
        "code_changed": super::code::code_changed(&loaded.nodes, &after),
        "scratch_class": scratch,
        "opt_ins_run": opt_ins_run,
        "warnings": warnings,
    });
    // The map is written and compiled; an opt-in that failed left the data it guards unready
    // (a counter behind the IDs, an empty index), so the call does not answer success.
    let failed: Vec<&String> = opt_ins_run
        .as_object()
        .into_iter()
        .flatten()
        .filter(|(_, v)| v.get("error").is_some())
        .map(|(k, _)| k)
        .collect();
    if !failed.is_empty() {
        let mut err =
            body(
                "STORAGE_MAP_OPT_IN_FAILED",
                format!(
                "the map was written and compiled, but {} failed: {}. Run it again, or fix the \
                 data by hand, before the table takes rows",
                failed.iter().map(|k| k.as_str()).collect::<Vec<_>>().join(" and "),
                opt_ins_run
            ),
                answer,
            );
        err["written"] = json!(true);
        return Err(err);
    }
    Ok(answer)
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

/// The map audit record of an `iris_doc` put (FR-031, FR-030c), from the text IRIS held before
/// (`None` for a new class) and the text it holds now: the storage versions, the maps that
/// changed, and whether map code changed.
pub fn doc_write_record(class: &str, before: Option<&str>, after: &str) -> Value {
    // The version of a set of map blocks: one block's is what `read` reports for it; several are
    // hashed together. A text iad cannot parse has no version, and the record says so.
    let version = |blocks: &[(String, Vec<Node>)]| {
        let canonical: Vec<String> = blocks
            .iter()
            .map(|(_, nodes)| emit_body(&normalize(nodes.clone())))
            .collect();
        super::model::version_hex(&canonical.join("\n"))
    };
    let mut unparsed = Vec::new();
    let mut blocks_of = |label: &str, text: &str| match map_blocks(text) {
        Ok(b) => Some(b),
        Err(e) => {
            unparsed.push(format!("{label}: {}", e.reason));
            None
        }
    };
    let old = before.map(|t| blocks_of("before", t));
    let new = blocks_of("after", after);
    let empty = Vec::new();
    let new_blocks = new.as_ref().unwrap_or(&empty);
    let old_blocks = old.as_ref().and_then(|o| o.as_ref()).unwrap_or(&empty);
    let old_of = |name: &str| {
        old_blocks
            .iter()
            .find(|(n, _)| n == name)
            .map_or(&[][..], |(_, nodes)| nodes.as_slice())
    };
    let mut changed = Vec::new();
    for (name, nodes) in new_blocks {
        for m in changed_maps(old_of(name), nodes) {
            if let Some(map) = &m.name {
                changed.push(if new_blocks.len() > 1 {
                    format!("/storage/{name}/maps/{map}")
                } else {
                    format!("/maps/{map}")
                });
            }
        }
    }
    let code_changed = new_blocks
        .iter()
        .any(|(name, nodes)| super::code::code_changed(old_of(name), nodes));
    let mut record = map_write_record(
        class,
        None,
        &match &old {
            Some(Some(b)) => version(b),
            _ => String::new(),
        },
        &new.as_ref().map(|b| version(b)).unwrap_or_default(),
        &changed,
        code_changed,
    );
    if !unparsed.is_empty() {
        record["unparsed"] = json!(unparsed);
    }
    record
}

// ───────────────────────────────────────────────────────────────────────────
// The iris_doc hand-off (FR-031, T078, T079)
// ───────────────────────────────────────────────────────────────────────────

/// The warning an `iris_doc` put that changed an SQL map carries.
pub const DOC_MAP_WARNING: &str =
    "SQL storage map changed: iris_storage_map edits maps with a version \
     check and keeps the class readable";

/// Whether `text` is class XML (an XML declaration or an `<Export>` root).
pub fn is_class_xml(text: &str) -> bool {
    crate::policy::code_edit_gate::looks_like_class_xml(text)
}

/// Whether any Storage block of `text` holds an `SQLMap`, of any storage type. Class XML is
/// read as `<SQLMap` inside a `<Storage>` element. A Storage block iad cannot parse counts when
/// the text names an `SQLMap` at all, so the safeguards see it and report the parse failure.
/// The text IRIS stores for `content`: `iris_doc` sends its lines, so a `\r\n` line ending
/// arrives as `\n`. The hand-off judges this text, never the raw content.
pub fn stored_form(content: &str) -> String {
    content.lines().collect::<Vec<_>>().join("\n")
}

/// The class text the hand-off judges: as IRIS will store it (FR-031). The lines rejoined
/// ([`stored_form`]); and when a mapped class is not in the form IRIS stores (an indented
/// `Storage` or map line, or a map no block reader takes), IRIS's own canonical text, read
/// through an uncompiled scratch copy. A class iad cannot have canonicalized is refused.
pub async fn as_iris_stores(
    iris: &IrisConnection,
    client: &reqwest::Client,
    ns: &str,
    content: &str,
) -> Result<String, Value> {
    let text = stored_form(content);
    if !content_has_sql_map(&text) || is_class_xml(&text) {
        return Ok(text);
    }
    let indented = text.lines().any(|l| {
        l.starts_with([' ', '\t']) && {
            let t = l.trim_start();
            t.starts_with("Storage ") || t.starts_with("<SQLMap")
        }
    });
    let readable = matches!(map_blocks(&text), Ok(b) if !b.is_empty());
    if readable && !indented {
        return Ok(text);
    }
    iris.canonical_class_text(&text, ns, client)
        .await
        .map_err(
            |e| match e.downcast_ref::<crate::iris::connection::ClassTextRefused>() {
                // IRIS refused the text itself: the error its own PUT gives, and nothing is stored.
                Some(refused) => body("UPLOAD_FAILED", refused.0.clone(), json!({"stored": false})),
                None => body(
                    "STORAGE_MAP_PARSE_FAILED",
                    format!(
                    "the class is not in the form IRIS stores, and iad could not have IRIS put it \
                     in that form to check its map: {e}. Nothing was written."
                ),
                    json!({}),
                ),
            },
        )
}

pub fn content_has_sql_map(text: &str) -> bool {
    let text = stored_form(text);
    let text = text.as_str();
    if is_class_xml(text) {
        return text.split("<Storage").skip(1).any(|after| {
            let inside = after.split("</Storage>").next().unwrap_or(after);
            inside.contains("<SQLMap")
        });
    }
    let in_blocks = match split_class(text) {
        Ok(spans) => spans.iter().any(|sp| {
            text[sp.body_start..sp.body_end]
                .lines()
                .any(|l| l.trim_start().starts_with("<SQLMap"))
        }),
        Err(_) => text.contains("<SQLMap"),
    };
    // A map after a `Storage` line the block reader did not take (an indented header) still goes
    // to the safeguards, which refuse a block they cannot read (fail closed).
    in_blocks || {
        let mut storage = false;
        text.lines().any(|l| {
            let l = l.trim_start();
            storage |= l.starts_with("Storage ");
            storage && l.starts_with("<SQLMap")
        })
    }
}

/// What an `iris_doc` put of a class with an SQL map leaves for after the PUT: the text IRIS held
/// before (none for a new class), the tree that was sent, and whether the map changed.
pub struct DocGuard {
    previous: Option<String>,
    /// The text of the put, which a restore may replace when the read-back saw nothing.
    content: String,
    /// Every Storage block of the put that holds a map, by name.
    sent: Vec<(String, Vec<Node>)>,
    changed: bool,
    /// What the data checks could only warn about (FR-023 without opt-ins).
    warnings: Vec<String>,
}

/// The FR-014 rules for an `iris_doc` put. Read-only storage is not refused here: `iris_doc`'s
/// handling of other storage types does not change (FR-032).
fn doc_validate(before: &[Node], after: &[Node]) -> Result<(), Value> {
    let Err(found) = super::validate::validate_change(before, after) else {
        return Ok(());
    };
    let left: Vec<_> = found
        .into_iter()
        .filter(|i| i.rule != "read_only")
        .collect();
    if left.is_empty() {
        Ok(())
    } else {
        Err(invalid_body(&left))
    }
}

/// The gates of the call in progress on `nodes`, judged as a write (fails closed outside one).
fn doc_gates(nodes: &[Node]) -> Option<Value> {
    let shape = super::CallShape {
        action: super::Action::Edit,
        unreadable: false,
        acknowledge_phi: false,
        seed_counter: false,
        build_indexes: false,
        include_values: false,
        code_changed: None,
    };
    match crate::tools::target::CALL_TARGET
        .try_with(|t| (t.primary.effective.clone(), t.primary.key.clone()))
    {
        Ok((policy, server)) => super::code::gate_tree(nodes, &shape, &policy, &server),
        Err(_) => Some(body(
            "TARGET_NOT_RESOLVED",
            "no call is in progress, so the storage-map gates cannot judge this put".into(),
            json!({}),
        )),
    }
}

/// The environment template and `allow` of the call's server, judging an `iris_doc` put as
/// Compile and Execute (FR-027). Fails closed outside a call, like [`doc_gates`].
fn doc_execute_gate() -> Option<Value> {
    use crate::iris::workspace_config::ToolCategory;
    let judged = [ToolCategory::Compile, ToolCategory::Execute];
    crate::tools::target::CALL_TARGET
        .try_with(|t| {
            let server = &t.primary;
            if let Err(gate) = crate::policy::gate::dispatch_gate_for(
                "iris_doc",
                &server.key,
                &server.policy,
                &judged,
                &Value::Null,
            ) {
                return Some(gate);
            }
            crate::iris::server_manager::policy_gate_for(
                "iris_doc",
                &server.key,
                &server.policy,
                &judged,
            )
        })
        .unwrap_or_else(|_| {
            Some(body(
                "TARGET_NOT_RESOLVED",
                "no call is in progress, so the storage-map gates cannot judge this put".into(),
                json!({}),
            ))
        })
}

/// Before an `iris_doc` PUT of `content` as `doc` (FR-031): class XML with a map is refused
/// (FR-012a); the map is validated (FR-014), gated (FR-026–FR-028) and, when the put compiles,
/// compiled in a scratch copy (FR-015). No version check and no confirm-removal.
pub async fn doc_precheck(
    iris: &IrisConnection,
    client: &reqwest::Client,
    doc: &str,
    content: &str,
    ns: &str,
    compile: bool,
) -> Result<DocGuard, Value> {
    if is_class_xml(content) {
        return Err(body(
            "STORAGE_MAP_INVALID",
            "class XML with an SQL storage map is refused: put the class as UDL, or edit the map \
             with iris_storage_map, so iad can check it"
                .into(),
            json!({}),
        ));
    }
    // FR-012a: every Storage block that holds a map, not only the first.
    let sent = match map_blocks(content) {
        Ok(blocks) if !blocks.is_empty() => blocks,
        Ok(_) => {
            return Err(body(
                "STORAGE_MAP_PARSE_FAILED",
                "the text names an SQLMap but iad finds no Storage block holding it".into(),
                json!({}),
            ))
        }
        Err(e) => {
            return Err(body(
                "STORAGE_MAP_PARSE_FAILED",
                format!("cannot parse the Storage block: {}", e.reason),
                json!({"offset": e.offset, "context": e.context}),
            ))
        }
    };
    let previous = get_doc(iris, client, doc, ns)
        .await
        .map_err(|e| body("IRIS_UNREACHABLE", e.to_string(), json!({})))?
        .map(|(text, _)| text);
    let mut warnings = Vec::new();
    let before_blocks = match previous.as_deref().map(all_blocks) {
        Some(Ok(blocks)) => blocks,
        Some(Err(e)) => {
            // Said, not hidden: every block is then judged as new.
            warnings.push(format!(
                "the class IRIS holds now has a Storage block iad cannot parse ({}), so the \
                 put was judged as if the class were new",
                e.reason
            ));
            Vec::new()
        }
        None => Vec::new(),
    };
    let before_of = |name: &str| {
        before_blocks
            .iter()
            .find(|(n, _)| n == name)
            .map(|(_, nodes)| nodes.clone())
            .unwrap_or_default()
    };
    for (name, nodes) in &sent {
        doc_validate(&before_of(name), nodes)?;
    }
    // FR-012a: every Storage block of the put, not only the maps'.
    if let Some(refusal) = gate_blocks(content, doc_gates) {
        return Err(refusal);
    }
    // FR-027: a put that adds or changes map code in any block is judged as Execute as well as
    // Compile, as an `iris_storage_map` edit is. Only a put can tell, because only it reads what
    // was there.
    if sent
        .iter()
        .any(|(name, nodes)| super::code::code_changed(&before_of(name), nodes))
    {
        if let Some(refusal) = doc_execute_gate() {
            return Err(refusal);
        }
    }
    let class = doc.strip_suffix(".cls").unwrap_or(doc);
    // FR-031 includes the data checks (FR-023); iris_doc has no opt-ins, so a refusal here is
    // lifted by making the change with iris_storage_map.
    for (name, nodes) in &sent {
        let plan = data_checks(
            &CheckCtx {
                iris,
                client,
                ns,
                class,
                seed_counter: false,
                build_indexes: false,
                warn_only: true,
            },
            &before_of(name),
            nodes,
        )
        .await?;
        warnings.extend(plan.warnings);
    }
    // The scratch compile protects the real compile, on the block the class uses: the one its
    // `StorageStrategy` names, else the first with a map (a scratch copy holds one Storage
    // block). A put without `compile` compiles nothing, so the compiled class keeps working
    // whatever the source says; storing a class with a new property before its map is the usual
    // way to add a field (#5502 until it is mapped).
    if compile {
        let used = storage_strategy(content)
            .and_then(|s| sent.iter().find(|(n, _)| *n == s))
            .unwrap_or(&sent[0]);
        scratch_check(iris, client, content, &used.1, ns, class).await?;
    }
    let changed = sent.iter().any(|(name, nodes)| {
        emit_body(&normalize(before_of(name))) != emit_body(&normalize(nodes.clone()))
    });
    Ok(DocGuard {
        previous,
        content: content.to_string(),
        sent,
        changed,
        warnings,
    })
}

/// The Storage block the `Class` line's `StorageStrategy` keyword names, if any.
fn storage_strategy(text: &str) -> Option<String> {
    let line = text.lines().find(|l| l.starts_with("Class "))?;
    parse_header(line).1.into_iter().find_map(|kw| {
        let (k, v) = kw.split_once('=')?;
        k.trim()
            .eq_ignore_ascii_case("StorageStrategy")
            .then(|| v.trim().trim_matches('"').to_string())
    })
}

impl DocGuard {
    /// After the PUT: what IRIS stored must be what was sent (FR-012) and pass the gates again
    /// (FR-012a). On a difference the previous text is put back, and the answer says so.
    pub async fn read_back(
        &self,
        iris: &IrisConnection,
        client: &reqwest::Client,
        doc: &str,
        ns: &str,
    ) -> Result<String, Value> {
        let stored = match get_doc(iris, client, doc, ns).await {
            Ok(Some((text, _))) => Some(text),
            _ => None,
        };
        let Err(mut err) = judge_stored(&self.sent, stored.as_deref(), doc, doc_gates) else {
            return Ok(stored.unwrap_or_default());
        };
        // What iad wrote: the read-back, or the text sent when there was none to read.
        let ours = stored.as_deref().unwrap_or(&self.content);
        match self.restore(iris, client, doc, ns, ours).await {
            Ok(()) => err["restored"] = json!(true),
            Err(e) => {
                err["restored"] = json!(false);
                err["restore_error"] = json!(e);
            }
        }
        Err(err)
    }

    /// Put back the text IRIS held before the PUT and compile it (FR-016), only while IRIS still
    /// holds `ours`, the version this put wrote (FR-010a). A new class has nothing to put back.
    pub async fn restore(
        &self,
        iris: &IrisConnection,
        client: &reqwest::Client,
        doc: &str,
        ns: &str,
        ours: &str,
    ) -> Result<(), String> {
        let Some(previous) = &self.previous else {
            return Err("the class is new, so there is no previous version to put back".into());
        };
        restore_over(iris, client, doc, previous, ns, ours).await
    }

    /// The audit record of this put, `written` being the text IRIS held after it (FR-030c).
    pub fn write_record(&self, class: &str, written: &str) -> Value {
        doc_write_record(class, self.previous.as_deref(), written)
    }

    /// The answer's warnings: the map-change warning when the map changed, and what the data
    /// checks found.
    pub fn warn_if_changed(&self, resp: &mut Value) {
        let mut all: Vec<&str> = Vec::new();
        if self.changed {
            all.push(DOC_MAP_WARNING);
        }
        all.extend(self.warnings.iter().map(String::as_str));
        if !all.is_empty() {
            resp["warnings"] = json!(all);
        }
    }
}

// ───────────────────────────────────────────────────────────────────────────
// Data checks before a write (FR-023, FR-023a, FR-023b; Phase 10)
// ───────────────────────────────────────────────────────────────────────────

/// The global node a storage's `SqlIdExpression` increments, when iad can tell for sure: a plain
/// `$i(^G)` or `$INCREMENT(^G(<literal subscripts>))`, any case. Anything else is not verifiable
/// (FR-024), and the `Err` says why.
pub fn id_counter_ref(
    keywords: &std::collections::BTreeMap<String, String>,
) -> Result<super::globals::GlobalRef, String> {
    let Some(expr) = keywords.get("SqlIdExpression") else {
        return Err("no SqlIdExpression: the caller supplies the IDs".into());
    };
    let e = expr.trim();
    let lower = e.to_ascii_lowercase();
    let Some(arg) = ["$increment(", "$i("]
        .iter()
        .find_map(|p| lower.starts_with(p).then(|| &e[p.len()..]))
        .and_then(|rest| rest.strip_suffix(')'))
        .map(str::trim)
    else {
        return Err(format!("{e} is not a plain $INCREMENT of a global"));
    };
    let Some(after_caret) = arg.strip_prefix('^') else {
        return Err(format!("{e} does not increment a global by name"));
    };
    let (name, subs) = match after_caret.find('(') {
        Some(i) => (&after_caret[..i], Some(&after_caret[i..])),
        None => (after_caret, None),
    };
    if crate::objectscript::validate_global_name(name).is_err() {
        return Err(format!("{e}: {name:?} is not a plain global name"));
    }
    let prefix = match subs {
        None => None,
        Some(s) => {
            let inner = s
                .strip_prefix('(')
                .and_then(|r| r.strip_suffix(')'))
                .ok_or_else(|| format!("{e}: unbalanced subscripts"))?;
            let parts: Vec<&str> = split_top_level(inner, ',')
                .into_iter()
                .map(str::trim)
                .collect();
            let literal = |t: &&str| {
                super::tree::is_canonical_number(t)
                    || (t.len() >= 2
                        && t.starts_with('"')
                        && t.ends_with('"')
                        && !t[1..t.len() - 1].replace("\"\"", "").contains('"'))
            };
            if inner.is_empty() || !parts.iter().all(literal) {
                return Err(format!(
                    "{e}: a subscript is not a literal, so iad cannot read it"
                ));
            }
            Some(format!("({})", parts.join(",")))
        }
    };
    Ok(super::globals::GlobalRef {
        name: format!("^{name}"),
        extended: None,
        subscripts_prefix: prefix,
    })
}

/// What the data checks found that the write must do after the real compile, and the warnings
/// for what they could not check.
#[derive(Debug, Default)]
pub struct DataPlan {
    /// `(counter reference, highest ID)` to seed (`seed_counter`).
    pub seed: Option<(String, String)>,
    /// Index maps to build (`build_indexes`).
    pub build: Vec<String>,
    pub warnings: Vec<String>,
}

/// A map's leading constant subscripts as `(1,"x")`, re-quoted by iad, or `None` when a leading
/// subscript is not a plain literal. Stops at the first subscript that is not constant.
fn constant_prefix(map: &super::model::SqlMap) -> Option<String> {
    let mut subs: Vec<&super::model::Subscript> = map.subscripts.iter().collect();
    subs.sort_by_key(|s| s.level);
    let mut parts = Vec::new();
    for sb in subs {
        let e = sb.expression.trim();
        if e.contains('{') {
            break;
        }
        if super::tree::is_canonical_number(e) {
            parts.push(e.to_string());
        } else {
            let inner = e.strip_prefix('"').and_then(|r| r.strip_suffix('"'))?;
            if inner.replace("\"\"", "").contains('"') {
                return None;
            }
            parts.push(crate::objectscript::os_str_expr(
                &inner.replace("\"\"", "\""),
            ));
        }
    }
    Some(if parts.is_empty() {
        String::new()
    } else {
        format!("({})", parts.join(","))
    })
}

/// A plain global name for generated code, or `None` (dynamic, extended, subscripted).
fn plain_global(name: &str) -> Option<String> {
    let g = super::globals::normalize_global(name).ok()?;
    let bare = g.name.strip_prefix('^')?;
    (g.extended.is_none()
        && g.subscripts_prefix.is_none()
        && crate::objectscript::validate_global_name(bare).is_ok())
    .then(|| format!("^{bare}"))
}

/// Run iad's own ObjectScript on the write's connection and return what it wrote.
async fn run_code(w: &CheckCtx<'_>, code: &str) -> Result<String, Value> {
    w.iris
        .execute_via_generator_prefixed(code, w.ns, w.client, "IrisDevSm")
        .await
        .map_err(|e| body("IRIS_EXECUTE_ERROR", format!("data check: {e}"), json!({})))
}

/// The maps of `after` that are new or differ from `before`.
fn changed_maps<'a>(before: &[Node], after: &'a [Node]) -> Vec<&'a Node> {
    after
        .iter()
        .filter(|n| n.tag == "SQLMap")
        .filter(|n| {
            let old = before
                .iter()
                .find(|b| b.tag == "SQLMap" && b.name == n.name);
            old.map(|o| emit_body(&normalize(vec![o.clone()])))
                != Some(emit_body(&normalize(vec![(*n).clone()])))
        })
        .collect()
}

/// What the data checks read from: the connection, the class, and whether the call opted in.
pub struct CheckCtx<'a> {
    pub iris: &'a IrisConnection,
    pub client: &'a reqwest::Client,
    pub ns: &'a str,
    pub class: &'a str,
    pub seed_counter: bool,
    pub build_indexes: bool,
    /// `iris_doc`, which has no opt-ins: an ID counter behind the data and an unbuilt index are
    /// warnings, not refusals, so a class can still be defined over existing globals. The walk
    /// check (FR-023b) still refuses: that map would return wrong rows.
    pub warn_only: bool,
}

/// A whole global, no extended reference and no subscripts.
fn whole_global_ref(name: &str) -> super::globals::GlobalRef {
    super::globals::GlobalRef {
        name: name.to_string(),
        extended: None,
        subscripts_prefix: None,
    }
}

/// Whether a map of a class other than the one being written uses `global` in the namespace.
/// A search that fails is the refusal, never "no other class": the opt-ins it guards rewrite
/// shared data (FR-023a).
async fn used_by_another_class(w: &CheckCtx<'_>, global: &str) -> Result<bool, Value> {
    super::uses::uses(
        w.iris,
        w.client,
        &whole_global_ref(global),
        &[w.ns.to_string()],
    )
    .await
    .map(|found| found.iter().any(|u| u.class != w.class))
    .map_err(|e| {
        body(
            "IRIS_UNREACHABLE",
            format!(
                "iad could not check which other classes use {global}, so it changed nothing: {e}"
            ),
            json!({}),
        )
    })
}

/// The data checks of a write (FR-023, FR-023a, FR-023b), on the real globals, before anything is
/// written. `Err` is the refusal; `Ok` says what the opt-ins must do afterwards.
pub async fn data_checks(
    w: &CheckCtx<'_>,
    before: &[Node],
    after: &[Node],
) -> Result<DataPlan, Value> {
    let mut plan = DataPlan::default();
    let m = super::model::from_tree(w.class, "", after);
    let old = super::model::from_tree(w.class, "", before);
    let changed: Vec<String> = changed_maps(before, after)
        .iter()
        .filter_map(|n| n.name.clone())
        .collect();
    walk_check(w, &m, &changed, &mut plan).await?;
    counter_check(w, &m, &old, &changed, &mut plan).await?;
    index_check(w, &m, &old, &mut plan).await?;
    Ok(plan)
}

/// FR-023b: a level the map walks as row IDs must not hold literal sub-nodes it does not skip.
async fn walk_check(
    w: &CheckCtx<'_>,
    m: &super::model::StorageModel,
    changed: &[String],
    plan: &mut DataPlan,
) -> Result<(), Value> {
    let mut sampled: std::collections::BTreeMap<String, super::shape::ShapeReport> =
        std::collections::BTreeMap::new();
    for map in m.storage.maps.iter().filter(|x| changed.contains(&x.name)) {
        let Some(g) = plain_global(&map.global) else {
            plan.warnings.push(format!(
                "map {}: its global {} is not a plain name, so its data was not checked",
                map.name, map.global
            ));
            continue;
        };
        if !sampled.contains_key(&g) {
            let report = super::sample::sample(
                w.iris,
                w.client,
                w.ns,
                &whole_global_ref(&g),
                super::sample::caps(None),
                false,
            )
            .await?;
            sampled.insert(g.clone(), report);
        }
        let shape = &sampled[&g];
        for sb in &map.subscripts {
            let walks = sb.expression.contains('{');
            let skips = sb.next_code.is_some()
                || sb.start_value.is_some()
                || !sb.invalid_conditions.is_empty();
            let Some(names) = shape
                .sub_nodes
                .get(&sb.level.to_string())
                .filter(|n| !n.is_empty())
            else {
                continue;
            };
            // Named nodes are a problem where they are not IDs: under row nodes that hold values
            // (a parent's "EV", D11), or mixed in among numeric IDs (Shelf's "reserved"). A
            // level of string IDs (Copy's B001) or an index value ("active") is not.
            let under_rows = sb.level >= 2
                && shape
                    .data_states
                    .get(&(sb.level - 1).to_string())
                    .and_then(|s| s.get("11"))
                    .is_some_and(|n| *n > 0);
            let among_numbers = shape
                .numeric_subscripts
                .get(&sb.level.to_string())
                .is_some_and(|n| *n > 0);
            if walks && !skips && (under_rows || among_numbers) {
                return Err(body(
                    "STORAGE_MAP_INVALID",
                    format!(
                        "map {} walks level {} of {g} as row IDs, and that level holds the \
                         named node(s) {}, which would read as rows. Skip them with a NextCode, \
                         a StartValue or an Invalidcondition (FR-023b).",
                        map.name,
                        sb.level,
                        names
                            .iter()
                            .map(|n| format!("\"{n}\""))
                            .collect::<Vec<_>>()
                            .join(", ")
                    ),
                    json!({"rules": [{"path": format!("/maps/{}/subscripts/{}", map.name, sb.level),
                                      "rule": "walks_literal_nodes"}],
                           "level": sb.level, "nodes": names}),
                ));
            }
        }
    }
    Ok(())
}

/// FR-023: the ID counter must not be behind the highest ID already stored. Checked when the
/// write sets the ID expression or the master map; a counter that was already behind is not
/// this write's doing, and refusing would block every unrelated edit.
async fn counter_check(
    w: &CheckCtx<'_>,
    m: &super::model::StorageModel,
    old: &super::model::StorageModel,
    changed: &[String],
    plan: &mut DataPlan,
) -> Result<(), Value> {
    let master = m.storage.maps.iter().find(|x| x.master);
    let id_touched = m.storage.keywords.get("SqlIdExpression")
        != old.storage.keywords.get("SqlIdExpression")
        || master.is_some_and(|x| changed.contains(&x.name));
    let (counter, master) = match (
        id_counter_ref(&m.storage.keywords),
        master.filter(|_| id_touched),
    ) {
        (Ok(counter), Some(master)) => (counter, master),
        (Err(why), _) if m.storage.keywords.contains_key("SqlIdExpression") => {
            plan.warnings
                .push(format!("the ID counter was not checked: {why}"));
            return Ok(());
        }
        _ => return Ok(()),
    };
    let first = master.subscripts.iter().min_by_key(|s| s.level);
    let g = match (plain_global(&master.global), first) {
        (Some(g), Some(sb)) if sb.expression.contains('{') => g,
        _ => {
            plan.warnings.push(
                "the highest ID could not be read from the master map, so the ID counter was \
                 not checked"
                    .into(),
            );
            return Ok(());
        }
    };
    let counter_ref = format!(
        "{}{}",
        counter.name,
        counter.subscripts_prefix.as_deref().unwrap_or("")
    );
    let c = crate::objectscript::os_str_expr(&counter_ref);
    let code = format!(
        "Set c={c},h=$Order({g}(\"\"),-1) For {{ Quit:h=\"\"  Quit:h=+h  Set h=$Order({g}(h),-1) }} Write $Get(@c),\"|\",h"
    );
    let out = run_code(w, &code).await?;
    // An answer without the separator is IRIS's error text: the check did not run, so the write
    // waits rather than passing unchecked.
    let Some((value, highest)) = out.trim().split_once('|') else {
        if w.warn_only {
            // iris_doc has no opt-ins: its ID checks warn (FR-031 as amended).
            plan.warnings.push(format!(
                "the ID counter {counter_ref} could not be checked against the highest ID in {g}; \
                 IRIS answered: {}",
                out.trim()
            ));
            return Ok(());
        }
        return Err(body(
            "STORAGE_MAP_INVALID",
            format!(
                "the ID counter {counter_ref} could not be checked against the highest ID in {g}; \
                 IRIS answered: {}",
                out.trim()
            ),
            json!({"rules": [{"path": "/keywords/SqlIdExpression", "rule": "counter_unreadable"}]}),
        ));
    };
    let behind = !highest.is_empty()
        && value.parse::<f64>().unwrap_or(0.0) < highest.parse::<f64>().unwrap_or(0.0);
    if !behind {
        return Ok(());
    }
    if w.warn_only {
        // Nothing to seed: iris_doc has no opt-in.
        plan.warnings.push(format!(
            "the ID counter {counter_ref} holds {value:?} but the highest ID in {g} is \
             {highest}: the next INSERT would overwrite a row (R5). Set the \
             counter, or use iris_storage_map edit with seed_counter: true."
        ));
        return Ok(());
    }
    if !w.seed_counter {
        return Err(body(
            "STORAGE_MAP_ID_REUSE",
            format!(
                "the ID counter {counter_ref} holds {value:?} but the highest ID in {g} is \
                 {highest}, so the next INSERT would overwrite a row (R5). \
                 Repeat with seed_counter: true to set the counter first."
            ),
            json!({"counter": counter_ref, "counter_value": value, "highest_id": highest}),
        ));
    }
    // FR-023a: another class counting with the same global must not be reseeded.
    if used_by_another_class(w, &counter.name).await? {
        return Err(body(
            "STORAGE_MAP_INVALID",
            format!(
                "seed_counter: another class's map uses {}, so iad will not reset it",
                counter.name
            ),
            json!({"rules": [{"path": "/keywords/SqlIdExpression", "rule": "counter_shared"}]}),
        ));
    }
    if super::tree::is_canonical_number(highest) {
        plan.seed = Some((c, highest.to_string()));
    } else {
        plan.warnings.push(format!(
            "the highest ID {highest:?} is not a number, so the counter was not seeded"
        ));
    }
    Ok(())
}

/// FR-023, FR-023a: a new index map over existing data needs its index built.
async fn index_check(
    w: &CheckCtx<'_>,
    m: &super::model::StorageModel,
    old: &super::model::StorageModel,
    plan: &mut DataPlan,
) -> Result<(), Value> {
    let master = m.storage.maps.iter().find(|x| x.master);
    let has_data = match master.and_then(|x| plain_global(&x.global)) {
        Some(g) => run_code(w, &format!("Write $Data({g})")).await?.trim() != "0",
        None => false,
    };
    for map in m.storage.maps.iter().filter(|x| {
        // An unbuilt extent bitmap is as wrong as an index: COUNT(*) reads it and answers 0.
        matches!(x.r#type.as_str(), "index" | "bitmap" | "bitmapextent")
            && !old.storage.maps.iter().any(|o| o.name == x.name)
    }) {
        let (Some(g), Some(prefix)) = (plain_global(&map.global), constant_prefix(map)) else {
            plan.warnings.push(format!(
                "index map {}: its range could not be checked",
                map.name
            ));
            continue;
        };
        let range = format!("{g}{prefix}");
        let filled = run_code(w, &format!("Write $Data({range})")).await?.trim() != "0";
        if w.build_indexes {
            let shared = used_by_another_class(w, &g).await?;
            if filled || shared {
                return Err(body(
                    "STORAGE_MAP_INVALID",
                    format!(
                        "build_indexes: {range} already holds nodes or another map uses it, and an \
                         index build purges the index first (FR-023a)"
                    ),
                    json!({"rules": [{"path": format!("/maps/{}", map.name), "rule": "index_range_in_use"}]}),
                ));
            }
            plan.build.push(map.name.clone());
        } else if has_data && !filled {
            if w.warn_only {
                plan.warnings.push(format!(
                    "index map {} is new and {range} is empty, but the table has rows: queries through \
                     it find nothing until the index is built (%BuildIndices, or iris_storage_map with \
                     build_indexes: true)",
                    map.name
                ));
            } else {
                return Err(body(
                    "STORAGE_MAP_INDEX_UNBUILT",
                    format!(
                        "index map {} is new and {range} is empty, but the table has rows: queries \
                         through it would find nothing (R5). Repeat with build_indexes: true.",
                        map.name
                    ),
                    json!({"map": map.name, "range": range}),
                ));
            }
        }
    }
    Ok(())
}

/// The opt-ins after the real compile (T094), as the call's execute identity: seed the counter
/// (reporting the value it replaced) and build the new indexes.
/// What the seed writes before the value it replaced, so its answer is told from an error.
const SEEDED: &str = "~IAD-SEEDED~";

async fn run_opt_ins(w: &WriteCtx<'_>, plan: &DataPlan) -> Value {
    let (iris, client) = w.exec.unwrap_or((w.iris, w.client));
    let mut done = serde_json::Map::new();
    if let Some((counter, highest)) = &plan.seed {
        // The marker tells a set counter from IRIS's error text, which also comes back as output.
        let code = format!("Set c={counter},old=$Get(@c),@c={highest} Write \"{SEEDED}\",old");
        done.insert(
            "seed_counter".into(),
            match iris
                .execute_via_generator_prefixed(&code, w.ns, client, "IrisDevSm")
                .await
            {
                Ok(out) => match out.trim().strip_prefix(SEEDED) {
                    Some(old) => json!({"set_to": highest, "replaced": old}),
                    None => json!({"error": out.trim()}),
                },
                Err(e) => json!({"error": e.to_string()}),
            },
        );
    }
    if !plan.build.is_empty() {
        let names: Vec<String> = plan
            .build
            .iter()
            .map(|n| crate::objectscript::os_str_expr(n))
            .collect();
        let code = format!(
            "Set sc=##class({}).%BuildIndices($lb({})) Write $Select(sc=1:\"OK\",1:$System.Status.GetErrorText(sc))",
            w.class,
            names.join(",")
        );
        done.insert(
            "build_indexes".into(),
            match iris
                .execute_via_generator_prefixed(&code, w.ns, client, "IrisDevSm")
                .await
            {
                Ok(out) if out.trim() == "OK" => json!({"built": plan.build}),
                Ok(out) => json!({"error": out.trim()}),
                Err(e) => json!({"error": e.to_string()}),
            },
        );
    }
    Value::Object(done)
}
