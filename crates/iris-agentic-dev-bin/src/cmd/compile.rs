use anyhow::{Context, Result};
use clap::Args;
use std::path::PathBuf;

use super::connection_args::ConnectionArgs;
use super::dispatch::dispatch_tool;

#[derive(Args)]
pub struct CompileCommand {
    /// .cls file(s) to compile directly, bypassing iris-dev.toml.
    /// With no files: reads iris-dev.toml (existing behavior).
    #[arg(value_name = "FILE")]
    pub files: Vec<PathBuf>,

    /// Route this call to a named registered IRIS instance instead of the default connection.
    #[arg(long)]
    pub server: Option<String>,

    #[command(flatten)]
    pub conn: ConnectionArgs,

    #[arg(long, default_value = "cuk")]
    pub flags: String,

    #[arg(long)]
    pub force_writable: bool,

    #[arg(long, default_value = "text")]
    pub format: String,
}

impl CompileCommand {
    pub async fn run(self) -> Result<()> {
        let namespace = self.conn.namespace.clone();
        let flags = self.flags.clone();
        let format = self.format.clone();

        let iris = self.conn.resolve().await?;

        if self.files.is_empty() {
            // Legacy toml-based compile (original behavior preserved). This is a
            // namespace-wide $SYSTEM.OBJ.CompileAll, not a single iris_compile call —
            // there's no tool method to delegate to here, so it stays direct.
            let client = iris_agentic_dev_core::iris::connection::IrisConnection::http_client()?;
            let target = ".";
            let code = format!(
                "Set sc=$SYSTEM.OBJ.CompileAll(\"{}\") If $System.Status.IsOK(sc) {{Write \"OK\"}} Else {{Write $System.Status.GetErrorText(sc)}}",
                flags
            );
            let out = iris
                .execute_via_generator(&code, &namespace, &client)
                .await
                .context("CompileAll failed")?;
            let out = out.trim();
            if out.ends_with("OK") || out == "OK" {
                let result =
                    serde_json::json!({"success": true, "target": target, "namespace": namespace});
                output_result(&result, &format);
            } else {
                let result = serde_json::json!({"success": false, "error_code": "IRIS_COMPILE_FAILED", "error": out, "target": target});
                output_result(&result, &format);
                std::process::exit(1);
            }
            return Ok(());
        }

        // File-args mode: delegate to iris_compile per file. iris_compile's own
        // is_local_path detection (target contains a path separator, or ends in .cls
        // and exists on disk) already does exactly what this used to hand-roll here —
        // upload via PUT, derive the class name from the file's `Class` declaration,
        // then compile. Passing the file path as `target` is enough; the tool method
        // does the read/upload/compile itself.
        let mut any_error = false;
        for path in &self.files {
            let target = path.to_string_lossy().to_string();
            let mut args = serde_json::json!({
                "target": target,
                "flags": flags,
                "namespace": namespace,
            });
            if let Some(server) = &self.server {
                args["server"] = serde_json::Value::String(server.clone());
            }
            if self.force_writable {
                args["force_writable"] = serde_json::Value::Bool(true);
            }

            match dispatch_tool(iris.clone(), "iris_compile", args).await {
                Ok(body) => {
                    let success = body["success"].as_bool().unwrap_or(false);
                    let doc_name = body["target"].as_str().unwrap_or(&target);
                    if format == "json" {
                        println!("{}", body);
                    } else if success {
                        println!("OK: {}", doc_name);
                    } else if let Some(errs) = body["errors"].as_array() {
                        for e in errs {
                            let text = e["text"].as_str().unwrap_or("");
                            println!("ERROR: {}: {}", doc_name, text);
                        }
                    } else if let Some(err) = body["error"].as_str() {
                        println!("ERROR: {}: {}", target, err);
                    }
                    if !success {
                        any_error = true;
                    }
                }
                Err(e) => {
                    eprintln!("error: compile failed for {}: {}", target, e);
                    any_error = true;
                }
            }
        }

        if any_error {
            std::process::exit(1);
        }
        Ok(())
    }
}

fn output_result(result: &serde_json::Value, format: &str) {
    if format == "json" {
        println!("{}", result);
    } else if result["success"] == true {
        println!("✓ Compiled: {}", result["target"].as_str().unwrap_or(""));
    } else {
        eprintln!(
            "error: [{}]: {}",
            result["error_code"].as_str().unwrap_or(""),
            result["error"].as_str().unwrap_or("")
        );
    }
}
