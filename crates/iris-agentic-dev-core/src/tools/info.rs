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
fn default_limit() -> usize {
    20
}

// ── iris_info ────────────────────────────────────────────────────────────────

#[derive(Debug, Deserialize, JsonSchema)]
pub struct InfoParams {
    /// What to fetch: documents, modified, namespace, metadata, jobs, csp_apps, csp_debug, sa_schema
    pub what: String,
    /// Document type filter for what=documents: CLS, MAC, INT, INC, CSP, ALL
    pub doc_type: Option<String>,
    /// Schema/cube name for what=sa_schema
    pub name: Option<String>,
    #[serde(default = "default_namespace")]
    pub namespace: String,
    /// If true, bypass the log store and return all results inline regardless of count.
    #[serde(default)]
    pub inline: bool,
}

pub async fn handle_iris_info(
    iris: &IrisConnection,
    client: &reqwest::Client,
    p: InfoParams,
    log_store: Arc<Mutex<log_store::LogStore>>,
) -> Result<rmcp::model::CallToolResult, rmcp::ErrorData> {
    let ns = &p.namespace;
    let url = match p.what.as_str() {
        "documents" => {
            // Bug 14: use versioned_ns_url so future API versions are used automatically.
            let cat = match p.doc_type.as_deref().unwrap_or("ALL") {
                "ALL" => "CLS".to_string(),
                t => t.to_uppercase(),
            };
            iris.versioned_ns_url(ns, &format!("/docnames/{}", cat))
        }
        "modified" => iris.versioned_ns_url(ns, "/modified/0"),
        "namespace" => iris.versioned_ns_url(ns, ""), // namespace metadata endpoint
        "metadata" => iris.atelier_url("/"), // root endpoint returns server metadata
        "jobs" => iris.versioned_ns_url(ns, "/jobs"),
        "csp_apps" => iris.versioned_ns_url(ns, "/cspapps"),
        "csp_debug" => iris.versioned_ns_url(ns, "/cspdebugid"),
        "sa_schema" => {
            let name = p.name.as_deref().unwrap_or("");
            iris.versioned_ns_url(ns, &format!("/saschema/{}", urlencoding::encode(name)))
        }
        other => return err_json("INVALID_PARAM", &format!("Unknown what='{}'. Use: documents, modified, namespace, metadata, jobs, csp_apps, csp_debug, sa_schema", other)),
    };

    let resp = client
        .get(&url)
        .basic_auth(&iris.username, Some(&iris.password))
        .send()
        .await
        .map_err(|e| rmcp::ErrorData::internal_error(format!("HTTP error: {e}"), None))?;

    if !resp.status().is_success() {
        return err_json(
            "IRIS_UNREACHABLE",
            &format!("HTTP {} for {}", resp.status(), url),
        );
    }

    let body: serde_json::Value = resp.json().await.unwrap_or_default();
    let mut result_json = serde_json::json!({"success": true, "what": p.what, "namespace": p.namespace, "result": body["result"]});

    // Progressive disclosure (027): for what=documents, truncate the document list.
    // The document names are in result["content"] — flatten to a top-level "documents" key.
    if p.what == "documents" {
        if let Some(content) = result_json["result"]["content"].as_array().cloned() {
            result_json["documents"] = serde_json::Value::Array(content);
            let threshold = log_store::read_inline_threshold("IRIS_INLINE_INFO", 30);
            log_store::apply_truncation(
                &mut result_json,
                "documents",
                threshold,
                p.inline,
                &log_store,
                "iris_info",
            );
        }
    }

    ok_json(result_json)
}

// ── iris_macro ───────────────────────────────────────────────────────────────

#[derive(Debug, Deserialize, JsonSchema)]
pub struct MacroParams {
    /// Action: list, signature, location, definition, expand
    pub action: String,
    pub name: Option<String>,
    #[serde(default)]
    pub args: Vec<String>,
    #[serde(default = "default_namespace")]
    pub namespace: String,
}

pub async fn handle_iris_macro(
    iris: &IrisConnection,
    client: &reqwest::Client,
    p: MacroParams,
) -> Result<rmcp::model::CallToolResult, rmcp::ErrorData> {
    match p.action.as_str() {
        "list" => {
            // Bug 14: use versioned_ns_url instead of hardcoded /v1/.
            let url = iris.versioned_ns_url(&p.namespace, "/docnames/INC");
            let resp = client
                .get(&url)
                .basic_auth(&iris.username, Some(&iris.password))
                .send()
                .await
                .map_err(|e| rmcp::ErrorData::internal_error(format!("HTTP error: {e}"), None))?;
            if !resp.status().is_success() {
                return ok_json(serde_json::json!({
                    "success": true,
                    "macros": [],
                    "note": "No include files found in this namespace"
                }));
            }
            let body: serde_json::Value = resp.json().await.unwrap_or_default();
            let inc_files: Vec<String> = body["result"]["content"]
                .as_array()
                .unwrap_or(&vec![])
                .iter()
                .filter_map(|v| v.as_str().map(|s| s.to_string()))
                .collect();
            ok_json(serde_json::json!({
                "success": true,
                "macros": inc_files,
                "note": "Lists .inc include files — macro definitions are found within these files"
            }))
        }
        action @ ("signature" | "location" | "definition" | "expand") => {
            let name = p.name.as_deref().unwrap_or("");
            let url = iris.versioned_ns_url(&p.namespace, "/action/getmacro");
            let arg_count = p.args.len();
            let resp = client
                .post(&url)
                .basic_auth(&iris.username, Some(&iris.password))
                .json(&serde_json::json!({
                    "macros": [{"name": name, "arguments": arg_count}],
                    "action": action,
                    "args": p.args,
                }))
                .send()
                .await
                .map_err(|e| rmcp::ErrorData::internal_error(format!("HTTP error: {e}"), None))?;
            let body: serde_json::Value = resp.json().await.unwrap_or_default();
            ok_json(
                serde_json::json!({"success": true, "name": name, "action": action, "result": body["result"]}),
            )
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
pub struct DebugParams {
    /// Action: map_int, error_logs, capture, source_map
    pub action: String,
    /// Error string for map_int e.g. "<UNDEFINED>x+3^MyApp.Foo.1"
    pub error_string: Option<String>,
    /// Class name for source_map
    pub class_name: Option<String>,
    #[serde(default = "default_limit")]
    pub limit: usize,
    #[serde(default = "default_namespace")]
    pub namespace: String,
}

pub async fn handle_iris_debug(
    iris: &IrisConnection,
    _client: &reqwest::Client,
    p: DebugParams,
) -> Result<rmcp::model::CallToolResult, rmcp::ErrorData> {
    let _query_url = iris.versioned_ns_url(&p.namespace, "/action/query");

    match p.action.as_str() {
        "map_int" => {
            let err = p.error_string.as_deref().unwrap_or("");
            let code = format!(
                "set err=\"{}\" set routine=$piece($piece(err,\"^\",2),\".\",1) set offset=$piece(err,\"+\",2) set offset=$piece(offset,\"^\",1) write ##class(%Studio.Debugger).SourceLine(routine,+offset)",
                err.replace('"', "\\\"")
            );
            match iris.execute(&code, &p.namespace).await {
                Ok(output) => ok_json(
                    serde_json::json!({"success": true, "error_string": err, "source_location": output.trim()}),
                ),
                Err(e) if e.to_string() == "DOCKER_REQUIRED" => ok_json(serde_json::json!({
                    "success": false, "error_code": "DOCKER_REQUIRED",
                    "error": "iris_debug map_int requires docker exec. Set IRIS_CONTAINER=<container_name>.",
                })),
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
                "note": "IRIS error log is not accessible via Atelier REST SQL. Set IRIS_CONTAINER to enable docker exec access to the full error log."
            }))
        }
        "capture" => {
            let code = "set err=$ZERROR write \"error:\"_err,! set loc=$ZPOSITION write \"position:\"_loc,!";
            match iris.execute(code, &p.namespace).await {
                Ok(output) => {
                    ok_json(serde_json::json!({"success": true, "capture": output.trim()}))
                }
                Err(e) if e.to_string() == "DOCKER_REQUIRED" => ok_json(serde_json::json!({
                    "success": false, "error_code": "DOCKER_REQUIRED",
                    "error": "iris_debug capture requires docker exec. Set IRIS_CONTAINER=<container_name>.",
                })),
                Err(e) => err_json("EXECUTION_FAILED", &e.to_string()),
            }
        }
        "source_map" => {
            let cls = p.class_name.as_deref().unwrap_or("");
            let code = format!(
                "set map=\"\" set line=1 do {{set int=##class(%Studio.Debugger).MapToINT(\"{cls}\",line,.intline) if int=\"\" quit set map=map_line_\"->\"_intline_\",\" set line=line+1 }} while 1 write map",
                cls = cls.replace('"', "\\\"")
            );
            match iris.execute(&code, &p.namespace).await {
                Ok(output) => ok_json(
                    serde_json::json!({"success": true, "class": cls, "mapping": output.trim()}),
                ),
                Err(e) if e.to_string() == "DOCKER_REQUIRED" => ok_json(serde_json::json!({
                    "success": false, "error_code": "DOCKER_REQUIRED",
                    "error": "iris_debug source_map requires docker exec. Set IRIS_CONTAINER=<container_name>.",
                })),
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
pub struct GenerateParams {
    /// What to generate — natural language description, e.g. "a Patient class with Name and DOB properties"
    pub description: String,
    /// Type: "class" (default) or "test"
    #[serde(default = "default_type")]
    pub gen_type: String,
    /// Existing class name to generate tests for (gen_type=test only)
    pub class_name: Option<String>,
    #[serde(default = "default_namespace")]
    pub namespace: String,
}

fn default_type() -> String {
    "class".to_string()
}

pub async fn handle_iris_generate(
    iris: &IrisConnection,
    client: &reqwest::Client,
    p: GenerateParams,
) -> Result<rmcp::model::CallToolResult, rmcp::ErrorData> {
    let ns = &p.namespace;
    let query_url = iris.versioned_ns_url(ns, "/action/query");

    match p.gen_type.as_str() {
        "test" => {
            let cls = p.class_name.as_deref().unwrap_or("");

            // Fetch the class's methods and properties as generation context
            let sql = format!(
                "SELECT Name, FormalSpec, ReturnType, Description \
                 FROM %Dictionary.CompiledMethod WHERE parent = '{}' ORDER BY Name",
                cls.replace('\'', "''")
            );
            let resp = client
                .post(&query_url)
                .basic_auth(&iris.username, Some(&iris.password))
                .json(&serde_json::json!({"query": sql}))
                .send()
                .await
                .map_err(|e| rmcp::ErrorData::internal_error(format!("HTTP error: {e}"), None))?;
            let body: serde_json::Value = resp.json().await.unwrap_or_default();
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
            let body: serde_json::Value = resp.json().await.unwrap_or_default();
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
pub struct TableInfoParams {
    /// SQL table name in Schema.Table format (e.g. "SQLUser.MyTable" or "MyApp.Orders").
    pub table: String,
    /// IRIS namespace to query. Defaults to "USER".
    #[serde(default = "crate::tools::default_namespace")]
    pub namespace: String,
    /// Include approximate row count (runs SELECT COUNT(*) — may be slow on large tables).
    #[serde(default)]
    pub include_row_count: bool,
}

pub async fn handle_iris_table_info(
    iris: &crate::iris::connection::IrisConnection,
    client: &reqwest::Client,
    p: TableInfoParams,
) -> Result<rmcp::model::CallToolResult, rmcp::ErrorData> {
    // Split "Schema.Table" → (schema, table). Tables with no dot use SQLUser schema.
    let (sql_schema, sql_table) = match p.table.find('.') {
        Some(idx) => (p.table[..idx].to_string(), p.table[idx + 1..].to_string()),
        None => ("SQLUser".to_string(), p.table.clone()),
    };

    // Look up class projection: find a compiled class whose SQL mapping matches.
    let lookup_code = format!(
        r#"
set sqlSchema = "{schema}", sqlTable = "{table}"
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
        schema = sql_schema.replace('"', "\\\""),
        table = sql_table.replace('"', "\\\""),
    );

    let output = iris
        .execute_via_generator(&lookup_code, &p.namespace, client)
        .await
        .map_err(|e| rmcp::ErrorData::internal_error(format!("execute failed: {e}"), None))?;

    let lines: std::collections::HashMap<&str, &str> =
        output.lines().filter_map(|l| l.split_once(':')).collect();

    if output.trim() == "NOT_FOUND" {
        return crate::tools::ok_json(serde_json::json!({
            "success": false,
            "error": format!("Table '{}' not found in namespace '{}'", p.table, p.namespace),
            "table": p.table,
            "namespace": p.namespace,
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
            "namespace": p.namespace,
            "data_global": if data_global.is_empty() { serde_json::Value::Null } else { data_global.into() },
            "index_global": if index_global.is_empty() { serde_json::Value::Null } else { index_global.into() },
            "accessible_from_embedded_python": true,
        });

        if p.include_row_count {
            let count = get_row_count(iris, client, &p.namespace, &sql_schema, &sql_table).await;
            obj["row_count"] = count;
        }
        obj
    } else {
        // DDL-created table — infer global names by IRIS naming convention
        let data_global = format!("^{}.{}D", sql_schema, sql_table);
        let index_global = format!("^{}.{}I", sql_schema, sql_table);
        let id_counter_global = format!("^{}.{}C", sql_schema, sql_table);

        let mut obj = serde_json::json!({
            "table": p.table,
            "type": "ddl_table",
            "namespace": p.namespace,
            "data_global": data_global,
            "index_global": index_global,
            "id_counter_global": id_counter_global,
            "accessible_from_embedded_python": true,
        });

        if p.include_row_count {
            let count = get_row_count(iris, client, &p.namespace, &sql_schema, &sql_table).await;
            obj["row_count"] = count;
        }
        obj
    };

    crate::tools::ok_json(serde_json::json!({
        "success": true,
        "result": result,
    }))
}

async fn get_row_count(
    iris: &crate::iris::connection::IrisConnection,
    client: &reqwest::Client,
    namespace: &str,
    schema: &str,
    table: &str,
) -> serde_json::Value {
    let code = format!(
        r#"set rs = ##class(%SQL.Statement).%ExecDirect(,"SELECT COUNT(*) FROM ""{schema}"".{table}")
if rs.%Next() {{ write rs.%GetData(1),! }} else {{ write "error",! }}"#,
        schema = schema.replace('"', "\\\""),
        table = table.replace('"', "\\\""),
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
        assert_eq!(p.namespace, "USER");
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
        assert_eq!(p.namespace, "USER");
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
        assert_eq!(p.namespace, "USER");
        assert!(p.error_string.is_none());
        assert!(p.class_name.is_none());
        assert!(p.limit > 0);
    }

    #[test]
    fn test_generate_params_defaults() {
        let p: GenerateParams =
            serde_json::from_str(r#"{"description": "A patient class"}"#).unwrap();
        assert_eq!(p.gen_type, "class");
        assert_eq!(p.namespace, "USER");
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
        assert_eq!(p.namespace, "USER");
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
