use anyhow::Result;
use clap::Args;
use iris_agentic_dev_core::{
    iris::connection::IrisConnection,
    tools::{param_check::nearest, CatalogueEntry, IrisTools, Toolset},
};
use std::time::Instant;

use super::connection_args::ConnectionArgs;

/// Sorted list of all tool names available in the Merged toolset.
/// Must stay in sync with `IrisTools::registered_tool_names(Toolset::Merged)`.
/// The T032 unit test enforces that parity — but parity with the tool *registry* says
/// nothing about whether `IrisTools::call_for_test()` (this CLI's dispatcher, distinct
/// from the MCP tool_router) actually has an arm for each name. That second relation is
/// covered separately by `test_all_tool_names_dispatch_in_call_for_test` in
/// tests/unit/test_tool_dispatch.rs — added after a field report found 22 names here with
/// no dispatch arm, so `iris-agentic-dev tool <name>` rejected them as "unknown tool" while
/// the MCP stdio transport served them correctly.
pub const TOOL_NAMES: &[&str] = &[
    "agent_history",
    "agent_stats",
    "capability_matrix",
    "check_config",
    "compare_document",
    "compare_namespace",
    "docs_introspect",
    "extract_message_map_routing",
    "find_subclass_implementations",
    "global_kill",
    "global_preview",
    "hl7_schema_inspect",
    "hl7_schema_list",
    "iris_add_server",
    "iris_admin",
    "iris_business_rule_info",
    "iris_compile",
    "iris_containers",
    "iris_coverage",
    "iris_credential_list",
    "iris_credential_manage",
    "iris_database_list",
    "iris_database_stats",
    "iris_debug",
    "iris_doc",
    "iris_doc_search",
    "iris_execute",
    "iris_execute_method",
    "iris_generate",
    "iris_generate_class",
    "iris_generate_test",
    "iris_get_log",
    "iris_global",
    "iris_import_servers",
    "iris_info",
    "iris_interop_query",
    "iris_lookup_manage",
    "iris_lookup_transfer",
    "iris_macro",
    "iris_message_body",
    "iris_mirror_status",
    "iris_namespace_create",
    "iris_namespace_list",
    "iris_production",
    "iris_production_diff",
    "iris_production_item",
    "iris_query",
    "iris_reload_pool",
    "iris_remove_server",
    "iris_search",
    "iris_servers",
    "iris_source_control",
    "iris_symbols",
    "iris_symbols_local",
    "iris_system_performance",
    "iris_table_info",
    "iris_test",
    "iris_test_server",
    "iris_ws_close",
    "iris_ws_exec",
    "iris_ws_open",
    "journal_search",
    "kb",
    "kb_index",
    "kb_recall",
    "mermaid_class",
    "mermaid_production",
    "my_access",
    "query_audit_log",
    "resolve_dynamic_dispatch",
    "resolve_storage",
    "skill",
    "skill_community",
    "skill_community_list",
    "skill_describe",
    "skill_forget",
    "skill_list",
    "skill_search",
    "stream_inspect",
    "telemetry_export_trace",
    "telemetry_query",
];

/// Returns the set of tool names covered by the dispatch map (== TOOL_NAMES).
pub fn dispatch_map_keys() -> std::collections::HashSet<&'static str> {
    TOOL_NAMES.iter().copied().collect()
}

#[derive(Args)]
pub struct ToolCommand {
    /// Exact MCP tool name (e.g. iris_info, iris_execute). Omit it with --list.
    #[arg(value_name = "TOOL_NAME")]
    pub name: Option<String>,

    /// JSON object of tool arguments (default: `{}`)
    #[arg(long, short = 'a', value_name = "JSON", default_value = "{}")]
    pub args: String,

    /// Wrap output in a stable JSON envelope {ok, tool, run_id, elapsed_ms, result, error}
    #[arg(long)]
    pub envelope: bool,

    /// List every tool this CLI can dispatch, one per line, with a one-line summary.
    /// Needs no IRIS connection.
    #[arg(long, conflicts_with = "schema")]
    pub list: bool,

    /// Print TOOL_NAME's description and inputSchema — the same contract tools/list serves.
    /// Needs no IRIS connection.
    #[arg(long)]
    pub schema: bool,

    /// Emit --list / --schema output as a single JSON document.
    #[arg(long)]
    pub json: bool,

    /// Which tool tier to list and dispatch: baseline, nostub, or merged.
    #[arg(long, env = "IRIS_TOOLSET", default_value = "merged")]
    pub toolset: String,

    #[command(flatten)]
    pub conn: ConnectionArgs,
}

impl ToolCommand {
    pub async fn run(self) -> Result<()> {
        if self.list || self.schema {
            return self.discover();
        }
        let Some(name) = self.name.clone() else {
            eprintln!("error: a tool name is required; run `iris-agentic-dev tool --list` to see");
            eprintln!("       every tool this CLI can dispatch, or pass --schema with a name");
            std::process::exit(1);
        };
        let envelope = self.envelope;
        let run_id = std::env::var("GAUNTLET_RUN_ID")
            .ok()
            .filter(|v| !v.is_empty());

        // Validate tool name before connecting
        if !TOOL_NAMES.contains(&name.as_str()) {
            if envelope {
                println!(
                    "{}",
                    serde_json::json!({
                        "ok": false,
                        "tool": name,
                        "run_id": run_id,
                        "elapsed_ms": 0,
                        "result": null,
                        "error": format!("unknown tool '{name}'")
                    })
                );
            } else {
                eprintln!("error: unknown tool '{}'", name);
                eprintln!("available tools:");
                for t in TOOL_NAMES {
                    eprintln!("  {}", t);
                }
            }
            std::process::exit(1);
        }

        // Parse args JSON
        let args_json: serde_json::Value = serde_json::from_str(&self.args)
            .map_err(|e| {
                if envelope {
                    println!(
                        "{}",
                        serde_json::json!({
                            "ok": false,
                            "tool": name,
                            "run_id": run_id,
                            "elapsed_ms": 0,
                            "result": null,
                            "error": format!("--args is not valid JSON: {e}")
                        })
                    );
                } else {
                    eprintln!("error: --args is not valid JSON: {}", e);
                }
                std::process::exit(1);
            })
            .unwrap();

        // Named `server=` / pool-only tools can run from `[instance.*]` even when
        // there is no top-level host (operate-mode fleet). check_config / iris_servers
        // never need a live default connection.
        let allow_no_default = name == "check_config"
            || name == "iris_servers"
            || args_json
                .get("server")
                .and_then(|v| v.as_str())
                .map(|s| !s.is_empty())
                .unwrap_or(false)
            || (name == "iris_test_server"
                && args_json
                    .get("name")
                    .and_then(|v| v.as_str())
                    .map(|s| !s.is_empty())
                    .unwrap_or(false));

        let iris: Option<IrisConnection> = match self.conn.resolve().await {
            Ok(c) => Some(c),
            Err(e) if allow_no_default => {
                if !envelope {
                    eprintln!("warning: no default IRIS connection ({e}); using connection pool");
                }
                None
            }
            Err(e) => {
                if envelope {
                    println!(
                        "{}",
                        serde_json::json!({
                            "ok": false,
                            "tool": name,
                            "run_id": run_id,
                            "elapsed_ms": 0,
                            "result": null,
                            "error": e.to_string()
                        })
                    );
                } else {
                    eprintln!("error: {}", e);
                }
                std::process::exit(1);
            }
        };

        let tools = IrisTools::new_with_toolset(iris, Toolset::from_str(&self.toolset))?;
        let t0 = Instant::now();

        match tools.call_for_test(&name, args_json).await {
            Ok(result) => {
                let elapsed_ms = t0.elapsed().as_millis() as u64;
                if envelope {
                    // Collect all text content into a single JSON value
                    let parts: Vec<serde_json::Value> = result
                        .content
                        .iter()
                        .filter_map(|c| c.as_text())
                        .filter_map(|t| serde_json::from_str::<serde_json::Value>(&t.text).ok())
                        .collect();
                    let result_value = if parts.len() == 1 {
                        parts.into_iter().next().unwrap()
                    } else {
                        serde_json::Value::Array(parts)
                    };
                    let tool_ok =
                        result_value.get("success") != Some(&serde_json::Value::Bool(false));
                    println!(
                        "{}",
                        serde_json::json!({
                            "ok": tool_ok,
                            "tool": name,
                            "run_id": run_id,
                            "elapsed_ms": elapsed_ms,
                            "result": result_value,
                            "error": null
                        })
                    );
                    if !tool_ok {
                        std::process::exit(1);
                    }
                } else {
                    let mut tool_success = true;
                    for content in &result.content {
                        if let Some(text) = content.as_text() {
                            println!("{}", text.text);
                            // Exit 1 when the tool itself reports failure so shell/CI can gate on exit code.
                            if let Ok(v) = serde_json::from_str::<serde_json::Value>(&text.text) {
                                if v.get("success") == Some(&serde_json::Value::Bool(false)) {
                                    tool_success = false;
                                }
                            }
                        }
                    }
                    if !tool_success {
                        std::process::exit(1);
                    }
                }
            }
            Err(e) => {
                let elapsed_ms = t0.elapsed().as_millis() as u64;
                if envelope {
                    println!(
                        "{}",
                        serde_json::json!({
                            "ok": false,
                            "tool": name,
                            "run_id": run_id,
                            "elapsed_ms": elapsed_ms,
                            "result": null,
                            "error": e.to_string()
                        })
                    );
                } else {
                    eprintln!("error: {}", e);
                }
                std::process::exit(1);
            }
        }
        Ok(())
    }

    /// `--list` and `--schema`: the discovery half, which never touches a connection.
    ///
    /// A shell-only caller can read the whole surface for about 6 KB and one tool's contract for
    /// about 1 KB, instead of the ~104 KB an MCP `tools/list` costs before the first call. Both
    /// answers come from `IrisTools::tool_catalogue()`, which is the same list `tools/list` serves,
    /// so this cannot advertise a tool the server does not have.
    ///
    /// `IrisTools::new_with_toolset(None, …)` is the whole reason no connection is needed: every
    /// tool's name, description and schema is static, and only calling one requires IRIS.
    fn discover(&self) -> Result<()> {
        let entries = self.catalogue()?;

        if self.list {
            if let Some(name) = &self.name {
                eprintln!("error: --list takes no tool name (got '{name}')");
                eprintln!("       for one tool's contract: iris-agentic-dev tool {name} --schema");
                std::process::exit(1);
            }
            self.print_list(&entries);
            return Ok(());
        }

        let Some(name) = self.name.as_deref() else {
            eprintln!("error: --schema needs a tool name");
            eprintln!("       for the whole list: iris-agentic-dev tool --list");
            std::process::exit(1);
        };
        match entries.iter().find(|e| e.name == name) {
            Some(entry) => {
                self.print_schema(entry);
                Ok(())
            }
            None => {
                let known: std::collections::BTreeSet<String> =
                    entries.iter().map(|e| e.name.clone()).collect();
                eprintln!("error: unknown tool '{name}'");
                if let Some(suggestion) = nearest(name, &known) {
                    eprintln!("       did you mean '{suggestion}'?");
                }
                eprintln!("       for the whole list: iris-agentic-dev tool --list");
                std::process::exit(1);
            }
        }
    }

    /// The tools this CLI can both list and dispatch, in name order.
    ///
    /// Two filters, both load-bearing. The toolset decides what the router registers, so
    /// `IRIS_TOOLSET=baseline` lists the baseline tier rather than always reporting Merged — a
    /// harness that scopes the toolset and then reads a Merged listing calls tools that are not
    /// there. `TOOL_NAMES` decides what `call_for_test` can dispatch, so a tool the router has and
    /// this CLI cannot call is left out: advertising it would send the caller into "unknown tool",
    /// which is exactly the drift that shipped once already (22 names, no dispatch arm).
    fn catalogue(&self) -> Result<Vec<CatalogueEntry>> {
        let tools = IrisTools::new_with_toolset(None, Toolset::from_str(&self.toolset))?;
        let dispatchable = dispatch_map_keys();
        Ok(tools
            .tool_catalogue()
            .into_iter()
            .filter(|e| dispatchable.contains(e.name.as_str()))
            .collect())
    }

    fn print_list(&self, entries: &[CatalogueEntry]) {
        if self.json {
            println!(
                "{}",
                serde_json::json!({
                    "toolset": Toolset::from_str(&self.toolset).as_str(),
                    "count": entries.len(),
                    "tools": entries.iter().map(|e| serde_json::json!({
                        "name": e.name,
                        "summary": e.summary,
                    })).collect::<Vec<_>>(),
                })
            );
            return;
        }
        let width = entries.iter().map(|e| e.name.len()).max().unwrap_or(0);
        for entry in entries {
            println!("{:<width$}  {}", entry.name, entry.summary, width = width);
        }
    }

    fn print_schema(&self, entry: &CatalogueEntry) {
        if self.json {
            println!(
                "{}",
                serde_json::json!({
                    "name": entry.name,
                    "description": entry.description,
                    "inputSchema": entry.input_schema,
                })
            );
            return;
        }
        println!("{}", entry.name);
        if let Some(description) = &entry.description {
            println!("\n{description}");
        }
        println!(
            "\ninputSchema:\n{}",
            serde_json::to_string_pretty(&entry.input_schema)
                .unwrap_or_else(|_| entry.input_schema.to_string())
        );
    }
}
