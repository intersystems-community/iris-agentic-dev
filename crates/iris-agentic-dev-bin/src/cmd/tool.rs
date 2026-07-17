use anyhow::Result;
use clap::Args;
use iris_agentic_dev_core::{
    iris::connection::IrisConnection,
    tools::{IrisTools, Toolset},
};

use super::connection_args::ConnectionArgs;

/// Sorted list of all tool names available in the Merged toolset.
/// Must stay in sync with `IrisTools::registered_tool_names(Toolset::Merged)`.
/// The T032 unit test enforces parity at compile+test time.
pub const TOOL_NAMES: &[&str] = &[
    "check_config",
    "docs_introspect",
    "extract_message_map_routing",
    "find_subclass_implementations",
    "iris_admin",
    "iris_business_rule_info",
    "iris_compile",
    "iris_containers",
    "iris_credential_list",
    "iris_credential_manage",
    "iris_debug",
    "iris_doc",
    "iris_execute",
    "iris_execute_method",
    "iris_generate",
    "iris_generate_class",
    "iris_get_log",
    "iris_global",
    "iris_info",
    "iris_interop_query",
    "iris_lookup_manage",
    "iris_lookup_transfer",
    "iris_macro",
    "iris_message_body",
    "iris_production",
    "iris_production_diff",
    "iris_production_item",
    "iris_query",
    "iris_search",
    "iris_source_control",
    "iris_symbols",
    "iris_symbols_local",
    "iris_table_info",
    "iris_test",
    "kb",
    "resolve_dynamic_dispatch",
    "skill",
    "skill_community",
    "telemetry_export_trace",
    "telemetry_query",
];

/// Returns the set of tool names covered by the dispatch map (== TOOL_NAMES).
pub fn dispatch_map_keys() -> std::collections::HashSet<&'static str> {
    TOOL_NAMES.iter().copied().collect()
}

#[derive(Args)]
pub struct ToolCommand {
    /// Exact MCP tool name (e.g. iris_info, iris_execute)
    #[arg(value_name = "TOOL_NAME")]
    pub name: String,

    /// JSON object of tool arguments (default: `{}`)
    #[arg(long, short = 'a', value_name = "JSON", default_value = "{}")]
    pub args: String,

    #[command(flatten)]
    pub conn: ConnectionArgs,
}

impl ToolCommand {
    pub async fn run(self) -> Result<()> {
        let name = self.name.clone();

        // Validate tool name before connecting
        if !TOOL_NAMES.contains(&name.as_str()) {
            eprintln!("error: unknown tool '{}'", name);
            eprintln!("available tools:");
            for t in TOOL_NAMES {
                eprintln!("  {}", t);
            }
            std::process::exit(1);
        }

        // Parse args JSON
        let args_json: serde_json::Value = serde_json::from_str(&self.args)
            .map_err(|e| {
                eprintln!("error: --args is not valid JSON: {}", e);
                std::process::exit(1);
            })
            .unwrap();

        let iris: Option<IrisConnection> = match self.conn.resolve().await {
            Ok(c) => Some(c),
            Err(e) => {
                eprintln!("error: {}", e);
                std::process::exit(1);
            }
        };

        let tools = IrisTools::new_with_toolset(iris, Toolset::Merged)?;

        match tools.call_for_test(&name, args_json).await {
            Ok(result) => {
                let mut tool_success = true;
                for content in &result.content {
                    if let Some(text) = content.raw.as_text() {
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
            Err(e) => {
                eprintln!("error: {}", e);
                std::process::exit(1);
            }
        }
        Ok(())
    }
}
