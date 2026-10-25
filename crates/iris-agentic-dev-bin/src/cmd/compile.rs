use anyhow::Result;
use clap::Args;
use std::path::PathBuf;

use super::connection_args::ConnectionArgs;
use super::dispatch::{build_tools, call, exit_if_refused};

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

        if let Some(msg) = iris_agentic_dev_core::tools::compile_flags_problem(&flags) {
            anyhow::bail!("{msg}");
        }
        if self.files.is_empty() {
            // `compile` with no file used to run `$SYSTEM.OBJ.CompileAll` straight through
            // `execute_via_generator`, outside every gate. Naming the target is what the gates
            // and the audit entry need, so it is refused rather than widened.
            eprintln!(
                "error: `compile` needs at least one .cls file. A namespace-wide compile is not \
                 offered here; compile a package with `iris-agentic-dev tool iris_compile --args \
                 '{{\"target\":\"MyApp.*.cls\"}}'`."
            );
            crate::exit(1);
        }

        let iris = self.conn.resolve().await?;

        // File-args mode: delegate to iris_compile per file. iris_compile's own
        // is_local_path detection (target contains a path separator, or ends in .cls
        // and exists on disk) already does exactly what this used to hand-roll here —
        // upload via PUT, derive the class name from the file's `Class` declaration,
        // then compile. Passing the file path as `target` is enough; the tool method
        // does the read/upload/compile itself.
        let tools = build_tools(iris)?;
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

            match call(&tools, "iris_compile", args).await {
                Ok(body) => {
                    exit_if_refused(&body);
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
            crate::exit(1);
        }
        Ok(())
    }
}
