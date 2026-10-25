//! iris_info — namespace/document discovery via Atelier REST.
//! iris_macro — macro introspection.
//! iris_debug — debug tools via Atelier xecute + SQL.
//! iris_generate — LLM-based class/test generation.

use crate::iris::connection::IrisConnection;
use crate::tools::log_store;
use schemars::JsonSchema;
use serde::Deserialize;
use std::sync::{Arc, Mutex};

fn ok_json(v: serde_json::Value) -> Result<rmcp::model::CallToolResult, rmcp::ErrorData> {
    Ok(rmcp::model::CallToolResult::structured(v))
}
fn err_json(code: &str, msg: &str) -> Result<rmcp::model::CallToolResult, rmcp::ErrorData> {
    crate::tools::err_result(
        serde_json::json!({"success": false, "error_code": code, "error": msg}),
    )
}
fn default_limit() -> usize {
    20
}

// ── iris_info ────────────────────────────────────────────────────────────────

#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct InfoParams {
    /// What to fetch: documents, modified, namespace, metadata, jobs, csp_apps, csp_debug, sa_schema
    #[schemars(extend("enum" = [
        "documents",
        "modified",
        "namespace",
        "metadata",
        "jobs",
        "csp_apps",
        "csp_debug",
        "sa_schema",
    ]))]
    pub what: String,
    /// Document type filter for what=documents: CLS, MAC, INT, INC, CSP, ALL (default ALL)
    pub doc_type: Option<String>,
    /// XData namespace URL for what=sa_schema, e.g. http://www.intersystems.com/deepsee. Not a cube
    /// name: cubes are listed by %DeepSee.Utils %GetCubeList through iris_execute.
    pub name: Option<String>,
    /// IRIS namespace. Defaults to the connection namespace (IRIS_NAMESPACE).
    #[serde(default)]
    pub namespace: Option<String>,
    /// If true, bypass the log store and return all results inline regardless of count.
    #[serde(default)]
    pub inline: bool,
    /// Route this call to a named registered IRIS instance. If omitted, uses the default connection.
    #[serde(default)]
    pub server: Option<String>,
    /// what=documents: include iad's own `IrisDevTmp.*` scratch classes (hidden by default)
    #[serde(default)]
    pub include_scratch: bool,
}

/// What `sa_schema` takes and where cube discovery lives. Names no skill (129 rule).
pub const SA_SCHEMA_GUIDANCE: &str = "what=sa_schema returns the Studio Assist grammar for an \
XData namespace URL, so `name` must be that URL, e.g. http://www.intersystems.com/deepsee. It does \
not describe cubes. To list cubes run `Do ##class(%DeepSee.Utils).%GetCubeList(.list)` through \
iris_execute, and for one cube's dimensions and measures \
`Do ##class(%DeepSee.Utils).%GetDimensionList(\"<cube>\",.info)`.";

/// The error text when `name` cannot be an XData namespace URL, or `None` when it can.
pub fn sa_schema_name_problem(name: Option<&str>) -> Option<String> {
    let n = name.unwrap_or("").trim();
    let is_url = n.starts_with("http://") || n.starts_with("https://");
    // A `.` or `..` segment would leave the Studio Assist path once the HTTP client resolves it (spec 141 C5).
    let dot_segment = n.split('/').any(|s| s == "." || s == "..");
    if is_url && !dot_segment {
        return None;
    }
    let got = if n.is_empty() {
        "no name".to_string()
    } else {
        format!("name '{n}'")
    };
    Some(format!("sa_schema got {got}. {SA_SCHEMA_GUIDANCE}"))
}

/// The `what` value `precheck` guards. A const, not a literal: the enum census reads literals in
/// the handler's call chain as `what`'s values, and this check must not stand in for the match.
const SA_SCHEMA: &str = "sa_schema";
/// GETs while Atelier answers 202, 500 ms apart. A build of the deepsee grammar takes under a second.
const SA_SCHEMA_ATTEMPTS: u32 = 20;

/// The Atelier path for a `sa_schema` URL. Each segment is encoded and the slashes stay raw:
/// Atelier answers `/saschema/http%3A//host/x` with the grammar and `/saschema/http%3A%2F%2Fhost%2Fx`
/// with 404.
pub fn sa_schema_path(name: &str) -> String {
    let segments: Vec<String> = name
        .trim()
        .split('/')
        .map(|s| urlencoding::encode(s).into_owned())
        .collect();
    format!("/saschema/{}", segments.join("/"))
}

/// The error text when a built `sa_schema` URL does not stay under `prefix` (the Studio Assist path)
/// once the HTTP client has resolved its dot segments, or `None` when it does (spec 141 C5).
pub fn sa_schema_url_problem(url: &str, prefix: &str) -> Option<String> {
    let inside = match (reqwest::Url::parse(url), reqwest::Url::parse(prefix)) {
        (Ok(u), Ok(p)) => u.path().starts_with(p.path()),
        _ => false,
    };
    (!inside).then(|| format!("sa_schema URL {url} leaves the Studio Assist path"))
}

/// A refusal that needs no IRIS, so it reaches a caller before the server is resolved.
pub fn precheck(p: &InfoParams) -> Option<String> {
    (p.what == SA_SCHEMA)
        .then(|| sa_schema_name_problem(p.name.as_deref()))
        .flatten()
}

/// Null, `{}`, `[]` or `""`: IRIS answered but said nothing.
pub fn is_empty_result(v: &serde_json::Value) -> bool {
    match v {
        serde_json::Value::Null => true,
        serde_json::Value::Object(m) => m.is_empty(),
        serde_json::Value::Array(a) => a.is_empty(),
        serde_json::Value::String(t) => t.trim().is_empty(),
        _ => false,
    }
}

/// Most documents one `inline=true` answer carries (`IRIS_INFO_MAX_DOCUMENTS` overrides).
pub const DEFAULT_DOCUMENT_CEILING: usize = 500;

pub fn parse_document_ceiling(raw: Option<&str>) -> usize {
    raw.and_then(|v| v.trim().parse::<usize>().ok())
        .filter(|n| *n > 0)
        .unwrap_or(DEFAULT_DOCUMENT_CEILING)
}

/// Move the document list from `result.content` to a top-level `documents`, once; drop
/// `IrisDevTmp.*` unless asked; and, for `inline`, cut at `ceiling` and say so.
///
/// The list used to go out twice (`result.content` stayed), and `inline=true` had no cap at all:
/// USER's 83,546 names came back as about 20 MB per call (130 round 4).
pub fn shape_documents(
    result_json: &mut serde_json::Value,
    include_scratch: bool,
    inline: bool,
    ceiling: usize,
) {
    let Some(content) = result_json["result"]
        .as_object_mut()
        .and_then(|r| r.remove("content"))
    else {
        return;
    };
    let mut docs = match content {
        serde_json::Value::Array(a) => a,
        _ => Vec::new(),
    };
    if !include_scratch {
        let before = docs.len();
        docs.retain(|d| {
            !d["name"]
                .as_str()
                .is_some_and(crate::tools::doc::is_scratch_doc)
        });
        let hidden = before - docs.len();
        if hidden > 0 {
            result_json["scratch_hidden"] = serde_json::json!(hidden);
        }
    }
    if inline {
        let total = docs.len();
        let cut = total > ceiling;
        docs.truncate(ceiling);
        result_json["truncated"] = serde_json::json!(cut);
        result_json["total_count"] = serde_json::json!(total);
        if cut {
            result_json["hint"] = serde_json::json!(format!(
                "{total} documents, first {ceiling} shown. Narrow it with doc_type (CLS, MAC, INT, INC, CSP), or use iris_doc mode=list with a pattern such as \"MyApp.*\"."
            ));
        }
    }
    result_json["documents"] = serde_json::Value::Array(docs);
}

pub async fn handle_iris_info(
    iris: &IrisConnection,
    client: &reqwest::Client,
    p: InfoParams,
    log_store: Arc<Mutex<log_store::LogStore>>,
) -> Result<rmcp::model::CallToolResult, rmcp::ErrorData> {
    let ns = crate::tools::resolve_namespace(p.namespace.as_deref(), &iris.namespace);
    let url = match p.what.as_str() {
        "documents" => {
            // Bug 14: use versioned_ns_url so future API versions are used automatically.
            let cat = crate::tools::doc::docnames_route(p.doc_type.as_deref().unwrap_or("ALL"));
            iris.versioned_ns_url(ns, &format!("/docnames/{}", cat))
        }
        "modified" => iris.versioned_ns_url(ns, "/modified/0"),
        "namespace" => iris.versioned_ns_url(ns, ""), // namespace metadata endpoint
        "metadata" => iris.atelier_url("/"), // root endpoint returns server metadata
        "jobs" => iris.versioned_ns_url(ns, "/jobs"),
        "csp_apps" => iris.versioned_ns_url(ns, "/cspapps"),
        "csp_debug" => iris.versioned_ns_url(ns, "/cspdebugid"),
        "sa_schema" => {
            if let Some(msg) = sa_schema_name_problem(p.name.as_deref()) {
                return err_json("INVALID_PARAMS", &msg);
            }
            let url = iris.versioned_ns_url(ns, &sa_schema_path(p.name.as_deref().unwrap_or("")));
            if let Some(msg) = sa_schema_url_problem(&url, &iris.versioned_ns_url(ns, "/saschema/")) {
                return err_json("INVALID_PARAMS", &msg);
            }
            url
        }
        other => return err_json("INVALID_PARAM", &format!("Unknown what='{}'. Use: documents, modified, namespace, metadata, jobs, csp_apps, csp_debug, sa_schema", other)),
    };

    // A missing or stale grammar (fresh instance, or a cube class compiled since) makes Atelier
    // start a background build and answer 202 with an empty result. Ask again until it is built.
    let mut attempts = 0;
    let resp = loop {
        let resp = client
            .get(&url)
            .basic_auth(&iris.username, Some(&iris.password))
            .send()
            .await
            .map_err(|e| rmcp::ErrorData::internal_error(format!("HTTP error: {e}"), None))?;
        if p.what != "sa_schema" || resp.status() != reqwest::StatusCode::ACCEPTED {
            break resp;
        }
        attempts += 1;
        if attempts >= SA_SCHEMA_ATTEMPTS {
            return err_json(
                "SA_SCHEMA_PENDING",
                &format!(
                    "IRIS is still building the grammar for {url} after {attempts} tries. Call again in a few seconds."
                ),
            );
        }
        tokio::time::sleep(std::time::Duration::from_millis(500)).await;
    };

    if p.what == "sa_schema" && resp.status() == reqwest::StatusCode::NOT_FOUND {
        return err_json(
            "SA_SCHEMA_NOT_FOUND",
            &format!("IRIS has no grammar at {url}. {SA_SCHEMA_GUIDANCE}"),
        );
    }
    if !resp.status().is_success() {
        return err_json(
            "IRIS_UNREACHABLE",
            &format!("HTTP {} for {}", resp.status(), url),
        );
    }

    let body: serde_json::Value = crate::iris::http::json_or_default(resp).await;
    if p.what == "sa_schema" && is_empty_result(&body["result"]) {
        return err_json(
            "SA_SCHEMA_NOT_FOUND",
            &format!("IRIS returned an empty grammar for {url}. {SA_SCHEMA_GUIDANCE}"),
        );
    }
    let mut result_json = serde_json::json!({"success": true, "what": p.what, "namespace": ns, "result": body["result"]});

    // Progressive disclosure (027): for what=documents, truncate the document list.
    // The document names are in result["content"] — flatten to a top-level "documents" key.
    if p.what == "documents" && result_json["result"]["content"].is_array() {
        let ceiling =
            parse_document_ceiling(std::env::var("IRIS_INFO_MAX_DOCUMENTS").ok().as_deref());
        shape_documents(&mut result_json, p.include_scratch, p.inline, ceiling);
        if !p.inline {
            let threshold = log_store::read_inline_threshold("IRIS_INLINE_INFO", 30);
            log_store::apply_truncation(
                &mut result_json,
                "documents",
                threshold,
                false,
                &log_store,
                "iris_info",
            );
        }
    }

    ok_json(result_json)
}

// ── iris_macro ───────────────────────────────────────────────────────────────

#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct MacroParams {
    /// Action: list, signature, location, definition, expand
    #[schemars(extend("enum" = ["list", "signature", "location", "definition", "expand"]))]
    pub action: String,
    pub name: Option<String>,
    #[serde(default)]
    pub args: Vec<String>,
    /// Include files to resolve the macro in, e.g. ["EnsConstants"]. If omitted, iad finds the
    /// include that defines it. With action=list, lists the macros these includes define.
    #[serde(default)]
    pub includes: Option<Vec<String>>,
    /// IRIS namespace. Defaults to the connection namespace (IRIS_NAMESPACE).
    #[serde(default)]
    pub namespace: Option<String>,
    /// Route this call to a named registered IRIS instance. If omitted, uses the default connection.
    #[serde(default)]
    pub server: Option<String>,
}

/// Atelier route for a macro action. `list` has none: it reads `/docnames/RTN/INC` or
/// `/action/getmacrolist`.
pub fn macro_route(action: &str) -> Option<&'static str> {
    match action {
        "signature" => Some("/action/getmacrosignature"),
        "location" => Some("/action/getmacrolocation"),
        "definition" => Some("/action/getmacrodefinition"),
        "expand" => Some("/action/getmacroexpansion"),
        _ => None,
    }
}

/// Atelier URL for a macro call. The `getmacro*` actions arrived in API v2, so a connection still
/// on v1 (never probed, or the probe failed) asks v2 rather than a v1 route that is always 404.
pub fn macro_url(iris: &IrisConnection, ns: &str, path: &str) -> String {
    if iris.atelier_version == crate::iris::connection::AtelierVersion::V1 {
        iris.atelier_url(&format!("/v2/{}{}", urlencoding::encode(ns), path))
    } else {
        iris.versioned_ns_url(ns, path)
    }
}

/// Atelier wants the arguments as one string, `(a,b)`. An array answers `expansion: []`.
pub fn macro_arguments(args: &[String]) -> String {
    if args.is_empty() {
        String::new()
    } else {
        format!("({})", args.join(","))
    }
}

fn strip_inc(name: &str) -> String {
    name.strip_suffix(".inc")
        .or_else(|| name.strip_suffix(".INC"))
        .unwrap_or(name)
        .to_string()
}

/// Request body for every `getmacro*` action. Atelier requires a `docname` but does not resolve
/// includes from it (`Ens.Director.cls` alone gives `definition: []`), so the includes carry the
/// lookup and the docname is a placeholder.
pub fn macro_request_body(name: &str, includes: &[String], args: &[String]) -> serde_json::Value {
    let includes: Vec<String> = includes.iter().map(|i| strip_inc(i)).collect();
    serde_json::json!({
        "docname": "%IadMacroLookup.cls",
        "macroname": name,
        "includes": includes,
        "arguments": macro_arguments(args),
    })
}

/// `(document, line)` from a `getmacrolocation` answer, or `None` when the macro is not defined in
/// the includes searched (Atelier answers `document: ""`).
pub fn macro_location(body: &serde_json::Value) -> Option<(String, i64)> {
    let c = &body["result"]["content"];
    let doc = c["document"].as_str().filter(|d| !d.is_empty())?;
    Some((doc.to_string(), c["line"].as_i64().unwrap_or(0)))
}

/// The Atelier error text, if the answer carries one. `status.errors` and `console` are joined so
/// the cause (e.g. "Failure to compile include files") is not lost behind "Utility failed".
pub fn macro_atelier_error(body: &serde_json::Value) -> Option<String> {
    let errors = body["status"]["errors"].as_array()?;
    if errors.is_empty() {
        return None;
    }
    let mut parts: Vec<String> = errors
        .iter()
        .filter_map(|e| e["error"].as_str().map(str::to_string))
        .collect();
    if let Some(console) = body["console"].as_array() {
        parts.extend(
            console
                .iter()
                .filter_map(|l| l.as_str().map(str::to_string)),
        );
    }
    Some(parts.join("; "))
}

/// Whether an Atelier error is the one a single uncompilable include causes. Given a list of
/// includes, Atelier compiles them all and fails the whole call if any one fails.
pub fn macro_include_compile_failure(err: &str) -> bool {
    err.contains("Failure to compile include files")
}

/// Include names (without `.inc`) from a `/docnames/RTN/INC` answer.
pub fn macro_include_names(body: &serde_json::Value) -> Vec<String> {
    body["result"]["content"]
        .as_array()
        .map(|a| {
            a.iter()
                .filter_map(|d| d["name"].as_str().or_else(|| d.as_str()))
                .map(strip_inc)
                .collect()
        })
        .unwrap_or_default()
}

/// Macro names an include defines, read from its source lines. `getmacrolist` cannot do this: it
/// ignores `includes` and always answers the same system list. A trailing `(` marks a macro that
/// takes arguments, as `getmacrolist` writes it.
pub fn macro_defines(lines: &[String]) -> Vec<String> {
    lines
        .iter()
        .filter_map(|l| {
            let l = l.trim_start();
            let rest = l
                .strip_prefix("#define")
                .or_else(|| l.strip_prefix("#def1arg"))
                .or_else(|| l.strip_prefix("#DEFINE"))
                .or_else(|| l.strip_prefix("#DEF1ARG"))?;
            if !rest.starts_with([' ', '\t']) {
                return None;
            }
            let token = rest.split_whitespace().next()?;
            match token.split_once('(') {
                Some((name, _)) => Some(format!("{name}(")),
                None => Some(token.to_string()),
            }
        })
        .collect()
}

/// `getmacrolocation` over `includes`, leaving out the ones that do not compile.
///
/// One broken include fails the call for the whole list (130 round 4), so on that failure the list
/// is halved until each broken include stands alone and is dropped. One broken include in 265
/// costs about 16 calls. Returns the location, if any, and the includes left out.
async fn macro_locate_around_broken(
    iris: &IrisConnection,
    client: &reqwest::Client,
    url: &str,
    name: &str,
    includes: &[String],
) -> Result<(Option<(String, i64)>, Vec<String>), String> {
    let mut skipped = Vec::new();
    let mut pending: Vec<&[String]> = vec![includes];
    while let Some(chunk) = pending.pop() {
        let body = macro_request_body(name, chunk, &[]);
        match macro_post(iris, client, url, &body).await {
            Ok(a) => {
                if let Some(found) = macro_location(&a) {
                    return Ok((Some(found), skipped));
                }
            }
            Err(e) if macro_include_compile_failure(&e) => {
                if chunk.len() == 1 {
                    skipped.push(chunk[0].clone());
                } else {
                    let (a, b) = chunk.split_at(chunk.len() / 2);
                    pending.push(b);
                    pending.push(a);
                }
            }
            Err(e) => return Err(e),
        }
    }
    Ok((None, skipped))
}

async fn macro_get(
    iris: &IrisConnection,
    client: &reqwest::Client,
    url: &str,
) -> Result<serde_json::Value, String> {
    let resp = client
        .get(url)
        .basic_auth(&iris.username, Some(&iris.password))
        .send()
        .await
        .map_err(|e| format!("HTTP error: {e}"))?;
    let status = resp.status();
    let body: serde_json::Value = crate::iris::http::json_or_default(resp).await;
    if !status.is_success() {
        return Err(format!("HTTP {status} from {url}"));
    }
    match macro_atelier_error(&body) {
        Some(e) => Err(e),
        None => Ok(body),
    }
}

async fn macro_post(
    iris: &IrisConnection,
    client: &reqwest::Client,
    url: &str,
    body: &serde_json::Value,
) -> Result<serde_json::Value, String> {
    let resp = client
        .post(url)
        .basic_auth(&iris.username, Some(&iris.password))
        .json(body)
        .send()
        .await
        .map_err(|e| format!("HTTP error: {e}"))?;
    let status = resp.status();
    let answer: serde_json::Value = crate::iris::http::json_or_default(resp).await;
    if !status.is_success() {
        return Err(format!("HTTP {status} from {url}"));
    }
    match macro_atelier_error(&answer) {
        Some(e) => Err(e),
        None => Ok(answer),
    }
}

pub async fn handle_iris_macro(
    iris: &IrisConnection,
    client: &reqwest::Client,
    p: MacroParams,
) -> Result<rmcp::model::CallToolResult, rmcp::ErrorData> {
    let ns = crate::tools::resolve_namespace(p.namespace.as_deref(), &iris.namespace);
    match p.action.as_str() {
        "list" => {
            if let Some(includes) = p.includes.as_deref().filter(|i| !i.is_empty()) {
                let mut macros = Vec::new();
                for inc in includes {
                    // The name becomes a URL path segment; `../x` would walk out of it (137).
                    if let Err(e) = crate::objectscript::validate_routine_name(&strip_inc(inc)) {
                        return crate::tools::err_result(e);
                    }
                    let path = format!("/doc/{}.inc", strip_inc(inc));
                    let body = match macro_get(iris, client, &macro_url(iris, ns, &path)).await {
                        Ok(b) => b,
                        Err(e) => {
                            return err_json(
                                "ATELIER_ERROR",
                                &format!("could not read include {}: {e}", strip_inc(inc)),
                            )
                        }
                    };
                    let lines: Vec<String> = body["result"]["content"]
                        .as_array()
                        .map(|a| {
                            a.iter()
                                .filter_map(|l| l.as_str().map(str::to_string))
                                .collect()
                        })
                        .unwrap_or_default();
                    macros.extend(macro_defines(&lines));
                }
                return ok_json(serde_json::json!({
                    "success": true,
                    "macros": macros,
                    "note": "Macros these includes define themselves (not the ones they #include). A trailing '(' marks one that takes arguments."
                }));
            }
            let url = macro_url(iris, ns, "/docnames/RTN/INC");
            match macro_get(iris, client, &url).await {
                Ok(body) => ok_json(serde_json::json!({
                    "success": true,
                    "macros": macro_include_names(&body),
                    "note": "Include files in this namespace. Pass includes=[...] to list the macros one defines."
                })),
                Err(e) => err_json("ATELIER_ERROR", &e),
            }
        }
        action @ ("signature" | "location" | "definition" | "expand") => {
            let Some(name) = p.name.as_deref().filter(|n| !n.is_empty()) else {
                return err_json(
                    "INVALID_PARAM",
                    &format!("action='{action}' needs name, e.g. name=\"ISERR\" (without $$$)"),
                );
            };
            let name = name.trim_start_matches("$$$");
            let loc_url = macro_url(iris, ns, "/action/getmacrolocation");

            // Find the include that defines the macro. System macros resolve with none; others
            // need theirs named. Given every include, getmacrolocation still answers, while
            // getmacrodefinition fails compiling them — so locate first, then ask in that one.
            let given: Vec<String> = p.includes.clone().unwrap_or_default();
            let mut skipped: Vec<String> = Vec::new();
            let mut includes = given.clone();
            let loc_body = macro_request_body(name, &includes, &[]);
            let mut location = match macro_post(iris, client, &loc_url, &loc_body).await {
                Ok(a) => macro_location(&a),
                Err(e) => return err_json("ATELIER_ERROR", &e),
            };
            let mut searched = if given.is_empty() {
                "system includes".to_string()
            } else {
                given.join(", ")
            };
            if location.is_none() && p.includes.is_none() {
                let inc_url = macro_url(iris, ns, "/docnames/RTN/INC");
                let all = match macro_get(iris, client, &inc_url).await {
                    Ok(body) => macro_include_names(&body),
                    Err(e) => return err_json("ATELIER_ERROR", &e),
                };
                (location, skipped) =
                    match macro_locate_around_broken(iris, client, &loc_url, name, &all).await {
                        Ok(found) => found,
                        Err(e) => return err_json("ATELIER_ERROR", &e),
                    };
                searched = format!("all {} include files in {ns}", all.len());
                if !skipped.is_empty() {
                    searched.push_str(&format!(
                        " except {}, which do not compile",
                        skipped.join(", ")
                    ));
                }
                if let Some((doc, _)) = &location {
                    includes = vec![strip_inc(doc)];
                }
            }
            let Some((document, line)) = location else {
                return err_json(
                    "MACRO_NOT_FOUND",
                    &format!(
                        "$$${name} is not defined in {searched}. Check the spelling, or pass the \
                         include that defines it as includes=[\"Name\"]."
                    ),
                );
            };

            let mut out = serde_json::json!({
                "success": true,
                "name": name,
                "action": action,
                "namespace": ns,
                "includes": includes,
                "document": document,
                "line": line,
            });
            if !skipped.is_empty() {
                out["skipped_includes"] = serde_json::json!(skipped);
            }
            if action != "location" {
                let url = macro_url(iris, ns, macro_route(action).unwrap_or_default());
                let body = macro_request_body(name, &includes, &p.args);
                let answer = match macro_post(iris, client, &url, &body).await {
                    Ok(a) => a,
                    Err(e) => return err_json("ATELIER_ERROR", &e),
                };
                let content = &answer["result"]["content"];
                match action {
                    "definition" => out["definition"] = content["definition"].clone(),
                    "signature" => out["signature"] = content["signature"].clone(),
                    _ => {
                        let expansion = content["expansion"].clone();
                        if expansion.as_array().is_some_and(|a| a.is_empty()) {
                            return err_json(
                                "MACRO_EXPANSION_EMPTY",
                                &format!(
                                    "$$${name} (defined in {document}:{line}) expanded to nothing \
                                     with args {:?}. Check its parameters with action=signature.",
                                    p.args
                                ),
                            );
                        }
                        out["expansion"] = expansion;
                    }
                }
            }
            ok_json(out)
        }
        other => err_json(
            "INVALID_PARAM",
            &format!(
                "Unknown action='{}'. Use: list, signature, location, definition, expand",
                other
            ),
        ),
    }
}

// ── iris_debug ───────────────────────────────────────────────────────────────

#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct DebugParams {
    /// Action: map_int, error_logs, capture, source_map
    #[schemars(extend("enum" = ["map_int", "error_logs", "capture", "source_map"]))]
    pub action: String,
    /// Error string for map_int e.g. "<UNDEFINED>x+3^MyApp.Foo.1"
    pub error_string: Option<String>,
    /// Class name for source_map
    pub class_name: Option<String>,
    #[serde(default = "default_limit")]
    pub limit: usize,
    /// IRIS namespace. Defaults to the connection namespace (IRIS_NAMESPACE).
    #[serde(default)]
    pub namespace: Option<String>,
    /// Route this call to a named registered IRIS instance. If omitted, uses the default connection.
    #[serde(default)]
    pub server: Option<String>,
}

pub async fn handle_iris_debug(
    iris: &IrisConnection,
    client: &reqwest::Client,
    p: DebugParams,
) -> Result<rmcp::model::CallToolResult, rmcp::ErrorData> {
    let ns = crate::tools::resolve_namespace(p.namespace.as_deref(), &iris.namespace);
    let _query_url = iris.versioned_ns_url(ns, "/action/query");

    match p.action.as_str() {
        "map_int" => {
            let err = p.error_string.as_deref().unwrap_or("");
            let code = format!(
                "set err={} set routine=$piece($piece(err,\"^\",2),\".\",1) set offset=$piece(err,\"+\",2) set offset=$piece(offset,\"^\",1) write ##class(%Studio.Debugger).SourceLine(routine,+offset)",
                crate::objectscript::os_str_expr(err)
            );
            match iris.execute_via_generator(&code, ns, client).await {
                Ok(output) => ok_json(
                    serde_json::json!({"success": true, "error_string": err, "source_location": output.trim()}),
                ),
                Err(e) => err_json("EXECUTION_FAILED", &e.to_string()),
            }
        }
        "error_logs" => {
            // IRIS error log tables (%SYSTEM.Error, %SYS.ErrorLog) are not SQL-accessible
            // via Atelier REST in IRIS Community edition.
            // Return empty list with a clear note rather than null.
            ok_json(serde_json::json!({
                "success": true,
                "logs": [],
                "note": "IRIS error log is not accessible via Atelier REST SQL."
            }))
        }
        "capture" => {
            let code = "set err=$ZERROR write \"error:\"_err,! set loc=$ZPOSITION write \"position:\"_loc,!";
            match iris.execute_via_generator(code, ns, client).await {
                Ok(output) => {
                    ok_json(serde_json::json!({"success": true, "capture": output.trim()}))
                }
                Err(e) => err_json("EXECUTION_FAILED", &e.to_string()),
            }
        }
        "source_map" => {
            let cls = p.class_name.as_deref().unwrap_or("");
            if let Err(e) = crate::objectscript::validate_class_name(cls) {
                return crate::tools::err_result(e);
            }
            let code = format!(
                "set map=\"\" set line=1 do {{set int=##class(%Studio.Debugger).MapToINT({cls_e},line,.intline) if int=\"\" quit set map=map_line_\"->\"_intline_\",\" set line=line+1 }} while 1 write map",
                cls_e = crate::objectscript::os_str_expr(cls)
            );
            match iris.execute_via_generator(&code, ns, client).await {
                Ok(output) => ok_json(
                    serde_json::json!({"success": true, "class": cls, "mapping": output.trim()}),
                ),
                Err(e) => err_json("EXECUTION_FAILED", &e.to_string()),
            }
        }
        other => err_json(
            "INVALID_PARAM",
            &format!(
                "Unknown action='{}'. Use: map_int, error_logs, capture, source_map",
                other
            ),
        ),
    }
}

// ── iris_generate ─────────────────────────────────────────────────────────────
//
// Context-provider design: returns everything the calling AI agent needs to
// write the class itself. No API key, no server-side LLM call, works with
// Copilot, Claude Code, or any MCP client.

#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct GenerateParams {
    /// Confirms the call on a `subject` server, where a write is refused until it is set. No
    /// effect on other servers.
    #[serde(default)]
    pub confirm: bool,
    /// What to generate — natural language description, e.g. "a Patient class with Name and DOB properties"
    pub description: String,
    /// Type: "class" (default) or "test"
    #[serde(default = "default_type")]
    #[schemars(extend("enum" = ["class", "test"]))]
    pub gen_type: String,
    /// Existing class name to generate tests for (gen_type=test only)
    pub class_name: Option<String>,
    /// IRIS namespace. Defaults to the connection namespace (IRIS_NAMESPACE).
    #[serde(default)]
    pub namespace: Option<String>,
    /// Route this call to a named registered IRIS instance. If omitted, uses the default connection.
    #[serde(default)]
    pub server: Option<String>,
}

fn default_type() -> String {
    "class".to_string()
}

pub async fn handle_iris_generate(
    iris: &IrisConnection,
    client: &reqwest::Client,
    p: GenerateParams,
) -> Result<rmcp::model::CallToolResult, rmcp::ErrorData> {
    let ns = crate::tools::resolve_namespace(p.namespace.as_deref(), &iris.namespace);
    let query_url = iris.versioned_ns_url(ns, "/action/query");

    match p.gen_type.as_str() {
        "test" => {
            let cls = p.class_name.as_deref().unwrap_or("");

            // Fetch the class's methods and properties as generation context
            let sql = "SELECT Name, FormalSpec, ReturnType, Description \
                 FROM %Dictionary.CompiledMethod WHERE parent = ? ORDER BY Name";
            let resp = client
                .post(&query_url)
                .basic_auth(&iris.username, Some(&iris.password))
                .json(&serde_json::json!({"query": sql, "parameters": [cls]}))
                .send()
                .await
                .map_err(|e| rmcp::ErrorData::internal_error(format!("HTTP error: {e}"), None))?;
            let body: serde_json::Value = crate::iris::http::json_or_default(resp).await;
            let methods = body["result"]["content"].clone();

            let prompt = format!(
                "Write an InterSystems IRIS %UnitTest.TestCase subclass to test '{}'. \
                 Requirements: {}. \
                 The class has these methods: {}. \
                 Rules: extend %UnitTest.TestCase, prefix test methods with 'Test', \
                 use $$$AssertEquals/$$$AssertTrue macros, include ##class({}).%New() in setup. \
                 Write only valid ObjectScript — no explanations, no markdown fences.",
                cls,
                p.description,
                serde_json::to_string(&methods).unwrap_or_default(),
                cls
            );

            ok_json(serde_json::json!({
                "success": true,
                "gen_type": "test",
                "target_class": cls,
                "namespace": ns,
                "prompt": prompt,
                "context": {
                    "methods": methods,
                    "suggested_class_name": format!("{}.Test", cls),
                },
                "instructions": "Use the prompt above to write the class, then call iris_doc(mode=put) to save it and iris_compile to compile it."
            }))
        }

        _ => {
            // Fetch existing classes in the namespace as naming/style context
            let sql = "SELECT TOP 10 Name FROM %Dictionary.ClassDefinition \
                       WHERE Name NOT LIKE '%\\%%' ESCAPE '\\' ORDER BY Name";
            let resp = client
                .post(&query_url)
                .basic_auth(&iris.username, Some(&iris.password))
                .json(&serde_json::json!({"query": sql}))
                .send()
                .await
                .map_err(|e| rmcp::ErrorData::internal_error(format!("HTTP error: {e}"), None))?;
            let body: serde_json::Value = crate::iris::http::json_or_default(resp).await;
            let existing: Vec<String> = body["result"]["content"]
                .as_array()
                .unwrap_or(&vec![])
                .iter()
                .filter_map(|r| r["Name"].as_str().map(|s| s.to_string()))
                .collect();

            // Detect likely package prefix from existing classes
            let package = existing
                .first()
                .and_then(|n| n.split('.').next())
                .unwrap_or("MyApp")
                .to_string();

            let prompt = format!(
                "Write an InterSystems IRIS ObjectScript class. \
                 Requirements: {}. \
                 Use package prefix '{}' to match existing classes in this namespace. \
                 Rules: valid ObjectScript syntax, extend %Persistent or %RegisteredObject \
                 as appropriate, include property definitions with types, add basic accessor \
                 methods if needed. Write only the class code — no explanations, no markdown fences.",
                p.description, package
            );

            ok_json(serde_json::json!({
                "success": true,
                "gen_type": "class",
                "namespace": ns,
                "prompt": prompt,
                "context": {
                    "existing_classes": existing,
                    "suggested_package": package,
                    "iris_version": iris.version.as_deref().unwrap_or("unknown"),
                },
                "instructions": "Use the prompt above to write the class, then call iris_doc(mode=put) to save it and iris_compile to compile it."
            }))
        }
    }
}

// ── iris_table_info ───────────────────────────────────────────────────────────

#[derive(Debug, serde::Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct TableInfoParams {
    /// SQL table name in Schema.Table format (e.g. "SQLUser.MyTable" or "MyApp.Orders").
    pub table: String,
    /// IRIS namespace to query. Defaults to the connection namespace (IRIS_NAMESPACE).
    #[serde(default)]
    pub namespace: Option<String>,
    /// Include approximate row count (runs SELECT COUNT(*) — may be slow on large tables).
    #[serde(default)]
    pub include_row_count: bool,
    /// Route this call to a named registered IRIS instance. If omitted, uses the default connection.
    #[serde(default)]
    pub server: Option<String>,
}

pub async fn handle_iris_table_info(
    iris: &crate::iris::connection::IrisConnection,
    client: &reqwest::Client,
    p: TableInfoParams,
) -> Result<rmcp::model::CallToolResult, rmcp::ErrorData> {
    let namespace = crate::tools::resolve_namespace(p.namespace.as_deref(), &iris.namespace);
    // The parts are spliced into ObjectScript and into SQL text below (137).
    if let Err(e) = crate::objectscript::validate_sql_identifier(&p.table) {
        return crate::tools::err_result(e);
    }
    let (sql_schema, sql_table) = table_schema_and_name(&p.table);

    // Look up class projection: find a compiled class whose SQL mapping matches.
    let lookup_code = format!(
        r#"
set sqlSchema = "{schema_lit}", sqlTable = "{table_lit}"
// Check table exists at all via INFORMATION_SCHEMA
set rsEx = ##class(%SQL.Statement).%ExecDirect(,"SELECT COUNT(*) FROM INFORMATION_SCHEMA.TABLES WHERE TABLE_SCHEMA = ? AND TABLE_NAME = ?", sqlSchema, sqlTable)
if rsEx.%Next() && (rsEx.%GetData(1) = 0) {{ write "NOT_FOUND",! quit }}
// Look for backing class
set rs = ##class(%SQL.Statement).%ExecDirect(,"SELECT c.Name, c.ClassType, s.DataLocation, s.IndexLocation, s.IDLocation FROM %Dictionary.CompiledClass c LEFT JOIN %Dictionary.CompiledStorage s ON s.parent = c.Name WHERE c.SqlSchemaName = ? AND c.SqlTableName = ?", sqlSchema, sqlTable)
if rs.%Next() {{
    write "CLASS:",rs.Name,!
    write "CLASSTYPE:",rs.ClassType,!
    write "DATA:",rs.DataLocation,!
    write "INDEX:",rs.IndexLocation,!
    write "ID:",rs.IDLocation,!
}} else {{
    write "DDL_TABLE",!
}}
"#,
        schema_lit = crate::objectscript::os_lit_inner(&sql_schema),
        table_lit = crate::objectscript::os_lit_inner(&sql_table),
    );

    let output = iris
        .execute_via_generator(&lookup_code, namespace, client)
        .await
        .map_err(|e| rmcp::ErrorData::internal_error(format!("execute failed: {e}"), None))?;

    // The DDL branch below infers global names from the table string alone, so it produces a
    // confident-looking answer for *any* output this function does not recognize. An IRIS error or
    // an empty generator result therefore came back as `success: true` with three invented global
    // names. Reject both before the branch runs.
    if let Some(msg) = crate::iris::connection::generator_error_message(&output) {
        return crate::tools::err_result(serde_json::json!({
            "success": false,
            "error_code": "TABLE_INFO_FAILED",
            "error": format!("Table lookup failed in namespace '{namespace}': {}", msg.trim()),
            "table": p.table,
            "namespace": namespace,
        }));
    }
    if output.trim().is_empty() {
        return crate::tools::err_result(serde_json::json!({
            "success": false,
            "error_code": "TABLE_INFO_EMPTY",
            "error": format!(
                "Table lookup for '{}' in namespace '{namespace}' returned no output. \
                 The lookup ran but wrote nothing, so nothing about this table is known.",
                p.table
            ),
            "table": p.table,
            "namespace": namespace,
        }));
    }

    let lines: std::collections::HashMap<&str, &str> =
        output.lines().filter_map(|l| l.split_once(':')).collect();

    if output.trim() == "NOT_FOUND" {
        return crate::tools::err_result(serde_json::json!({
            "success": false,
            "error": format!("Table '{}' not found in namespace '{}'", p.table, namespace),
            "table": p.table,
            "namespace": namespace,
        }));
    }

    let result = if lines.contains_key("CLASS") {
        // Class-projected table
        let class_name = lines.get("CLASS").copied().unwrap_or("").trim();
        let data_global = lines.get("DATA").copied().unwrap_or("").trim();
        let index_global = lines.get("INDEX").copied().unwrap_or("").trim();

        let mut obj = serde_json::json!({
            "table": p.table,
            "type": "class_projection",
            "class": class_name,
            "namespace": namespace,
            "data_global": if data_global.is_empty() { serde_json::Value::Null } else { data_global.into() },
            "index_global": if index_global.is_empty() { serde_json::Value::Null } else { index_global.into() },
            "accessible_from_embedded_python": true,
        });

        if p.include_row_count {
            let count = get_row_count(iris, client, namespace, &sql_schema, &sql_table).await;
            obj["row_count"] = count;
        }
        obj
    } else if output.lines().any(|l| l.trim() == "DDL_TABLE") {
        // DDL-created table — infer global names by IRIS naming convention
        let data_global = format!("^{}.{}D", sql_schema, sql_table);
        let index_global = format!("^{}.{}I", sql_schema, sql_table);
        let id_counter_global = format!("^{}.{}C", sql_schema, sql_table);

        let mut obj = serde_json::json!({
            "table": p.table,
            "type": "ddl_table",
            "namespace": namespace,
            "data_global": data_global,
            "index_global": index_global,
            "id_counter_global": id_counter_global,
            "accessible_from_embedded_python": true,
        });

        if p.include_row_count {
            let count = get_row_count(iris, client, namespace, &sql_schema, &sql_table).await;
            obj["row_count"] = count;
        }
        obj
    } else {
        // Neither `CLASS:` nor `DDL_TABLE` nor `NOT_FOUND`. The lookup wrote something, but nothing
        // this function can read — report that instead of guessing global names off the table name.
        return crate::tools::err_result(serde_json::json!({
            "success": false,
            "error_code": "TABLE_INFO_UNPARSEABLE",
            "error": format!(
                "Table lookup for '{}' in namespace '{namespace}' produced output that could not be \
                 parsed. Expected CLASS:, DDL_TABLE or NOT_FOUND.",
                p.table
            ),
            "table": p.table,
            "namespace": namespace,
            "raw_output": output.chars().take(500).collect::<String>(),
        }));
    };

    crate::tools::ok_json(serde_json::json!({
        "success": true,
        "result": result,
    }))
}

/// `Schema.Table` → (schema, table), each without its delimiting quotes. A name with no dot is in
/// `SQLUser`. Only a dot outside a delimited identifier separates the two, so `"A.B".C` is schema
/// `A.B` and table `C`.
pub fn table_schema_and_name(table: &str) -> (String, String) {
    let mut parts = crate::sql_lex::split_qualified_name(table);
    if parts.len() == 1 {
        return ("SQLUser".to_string(), parts.remove(0));
    }
    let schema = parts.remove(0);
    (schema, parts.join("."))
}

/// `name` as a SQL delimited identifier: quoted, with an embedded `"` doubled.
pub fn quote_sql_identifier(name: &str) -> String {
    format!("\"{}\"", name.replace('"', "\"\""))
}

async fn get_row_count(
    iris: &crate::iris::connection::IrisConnection,
    client: &reqwest::Client,
    namespace: &str,
    schema: &str,
    table: &str,
) -> serde_json::Value {
    let code = format!(
        r#"set rs = ##class(%SQL.Statement).%ExecDirect(,"SELECT COUNT(*) FROM {from_lit}")
if rs.%Next() {{ write rs.%GetData(1),! }} else {{ write "error",! }}"#,
        from_lit = crate::objectscript::os_lit_inner(&format!(
            "{}.{}",
            quote_sql_identifier(schema),
            quote_sql_identifier(table)
        )),
    );
    match iris.execute_via_generator(&code, namespace, client).await {
        Ok(out) => out
            .trim()
            .parse::<u64>()
            .map(serde_json::Value::from)
            .unwrap_or(serde_json::Value::Null),
        Err(_) => serde_json::Value::Null,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_info_params_defaults() {
        let p: InfoParams = serde_json::from_str(r#"{"what": "documents"}"#).unwrap();
        assert_eq!(p.namespace, None);
        assert_eq!(
            crate::tools::resolve_namespace(p.namespace.as_deref(), "APP"),
            "APP"
        );
        assert!(p.doc_type.is_none());
        assert!(p.name.is_none());
        assert!(!p.inline);
    }

    #[test]
    fn test_info_params_with_doc_type() {
        let p: InfoParams =
            serde_json::from_str(r#"{"what": "documents", "doc_type": "CLS"}"#).unwrap();
        assert_eq!(p.doc_type.as_deref(), Some("CLS"));
    }

    #[test]
    fn test_info_params_missing_what_fails() {
        let r: Result<InfoParams, _> = serde_json::from_str(r#"{}"#);
        assert!(r.is_err());
    }

    #[test]
    fn test_macro_params_defaults() {
        let p: MacroParams = serde_json::from_str(r#"{"action": "list"}"#).unwrap();
        assert_eq!(p.namespace, None);
        assert_eq!(
            crate::tools::resolve_namespace(p.namespace.as_deref(), "APP"),
            "APP"
        );
        assert!(p.name.is_none());
        assert!(p.args.is_empty());
    }

    #[test]
    fn test_macro_params_with_name_and_args() {
        let p: MacroParams =
            serde_json::from_str(r#"{"action": "expand", "name": "ISERR", "args": ["sc"]}"#)
                .unwrap();
        assert_eq!(p.action, "expand");
        assert_eq!(p.name.as_deref(), Some("ISERR"));
        assert_eq!(p.args, vec!["sc"]);
    }

    #[test]
    fn test_debug_params_defaults() {
        let p: DebugParams = serde_json::from_str(r#"{"action": "error_logs"}"#).unwrap();
        assert_eq!(p.namespace, None);
        assert_eq!(
            crate::tools::resolve_namespace(p.namespace.as_deref(), "APP"),
            "APP"
        );
        assert!(p.error_string.is_none());
        assert!(p.class_name.is_none());
        assert!(p.limit > 0);
    }

    #[test]
    fn test_generate_params_defaults() {
        let p: GenerateParams =
            serde_json::from_str(r#"{"description": "A patient class"}"#).unwrap();
        assert_eq!(p.gen_type, "class");
        assert_eq!(p.namespace, None);
        assert_eq!(
            crate::tools::resolve_namespace(p.namespace.as_deref(), "APP"),
            "APP"
        );
        assert!(p.class_name.is_none());
    }

    #[test]
    fn test_generate_params_test_type() {
        let p: GenerateParams = serde_json::from_str(
            r#"{"description": "tests for Foo", "gen_type": "test", "class_name": "Foo.Bar"}"#,
        )
        .unwrap();
        assert_eq!(p.gen_type, "test");
        assert_eq!(p.class_name.as_deref(), Some("Foo.Bar"));
    }

    #[test]
    fn test_table_info_params_defaults() {
        let p: TableInfoParams = serde_json::from_str(r#"{"table": "SQLUser.MyTable"}"#).unwrap();
        assert_eq!(p.namespace, None);
        assert_eq!(
            crate::tools::resolve_namespace(p.namespace.as_deref(), "APP"),
            "APP"
        );
        assert!(!p.include_row_count);
    }

    #[test]
    fn test_table_info_params_with_row_count() {
        let p: TableInfoParams =
            serde_json::from_str(r#"{"table": "Foo.Orders", "include_row_count": true}"#).unwrap();
        assert!(p.include_row_count);
    }

    #[test]
    fn test_table_info_params_missing_table_fails() {
        let r: Result<TableInfoParams, _> = serde_json::from_str(r#"{}"#);
        assert!(r.is_err());
    }
}
