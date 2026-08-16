use anyhow::Result;
use clap::Args;

use super::connection_args::ConnectionArgs;
use super::dispatch::dispatch_tool;
use super::tsv::{extract_columns, extract_rows, rows_to_tsv, tsv_header};

#[derive(Args)]
pub struct QueryCommand {
    /// SQL statement to execute
    #[arg(value_name = "SQL")]
    pub sql: String,

    /// Route this call to a named registered IRIS instance instead of the default connection.
    #[arg(long)]
    pub server: Option<String>,

    #[command(flatten)]
    pub conn: ConnectionArgs,
}

impl QueryCommand {
    pub async fn run(self) -> Result<()> {
        let namespace = self.conn.namespace.clone();
        let iris = self.conn.resolve().await?;

        let mut args = serde_json::json!({ "query": self.sql, "namespace": namespace });
        if let Some(server) = &self.server {
            args["server"] = serde_json::Value::String(server.clone());
        }

        let body = dispatch_tool(iris, "iris_query", args).await;
        let body = match body {
            Ok(b) => b,
            Err(e) => {
                eprintln!("error: {}", e);
                std::process::exit(1);
            }
        };

        if body["success"].as_bool() != Some(true) {
            let msg = body["error"].as_str().unwrap_or("query failed");
            eprintln!("error: {}", msg);
            std::process::exit(1);
        }

        // iris_query returns {"rows": [...], "count": N}, a flat array of row-objects —
        // not the raw Atelier {"result":{"content":[...]}} shape. Wrap it so the
        // existing tsv extraction helpers (written against the raw Atelier shape) work
        // unchanged, rather than duplicating their column/row logic for one caller.
        let rows = body["rows"].clone();
        let atelier_shaped = serde_json::json!({ "result": { "content": rows } });

        let cols = extract_columns(&atelier_shaped);
        if !cols.is_empty() {
            let col_refs: Vec<&str> = cols.iter().map(|s| s.as_str()).collect();
            println!("{}", tsv_header(&col_refs));
            let rows = extract_rows(&atelier_shaped);
            if !rows.is_empty() {
                print!("{}", rows_to_tsv(&rows));
            }
        }

        Ok(())
    }
}
